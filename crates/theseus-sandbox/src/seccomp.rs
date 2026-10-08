//! The seccomp filter (design §2.2), built by hand as classic BPF on `libc`
//! alone: `seccompiler` is not in the local registry, and a build fetches
//! nothing (§5, question 7's alternative). Its decisions are tested by
//! running the program in a small interpreter, and live by the contract.
//!
//! It allows by default, and denies a list with `EPERM`, never with a kill,
//! so a program gets an error it can report. Its shape follows Docker's
//! default profile:
//! - namespaces and mounts: `unshare`, `setns`, `mount`, `umount2`,
//!   `pivot_root`, the new mount API, and `clone` with any `CLONE_NEW*` flag.
//!   `clone3` gets `ENOSYS`, so the C library falls back to `clone`, whose
//!   flags the filter can read;
//! - the kernel: modules, `kexec_*`, `reboot`, swap, `acct`, quotas, and the
//!   time setters;
//! - introspection: `ptrace`, `process_vm_*`, `perf_event_open`, `bpf`,
//!   `userfaultfd`, the file-handle calls, `kcmp`, `pidfd_getfd`, and (x86)
//!   `iopl` and `ioperm`;
//! - keys: `keyctl`, `add_key`, and `request_key`;
//! - `io_uring_*`, whose operations would bypass the filter.
//!
//! A foreign ABI (i386 on x86_64, or x32) kills the process.

use std::io;

use libc::sock_filter;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // EM_X86_64 | 64-bit | little-endian
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7; // EM_AARCH64 | 64-bit | little-endian
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("L1's seccomp filter has tables for x86_64 and aarch64 only");

/// x86_64's x32 system calls carry this bit in their number.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

// struct seccomp_data: nr at 0, arch at 4, the instruction pointer at 8,
// and six 64-bit arguments from 16. The low half of an argument comes first
// on a little-endian machine.
const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;
#[cfg(target_endian = "little")]
const OFF_ARG0_LO: u32 = 16;
#[cfg(target_endian = "big")]
const OFF_ARG0_LO: u32 = 20;

pub const RET_ALLOW: u32 = 0x7fff_0000;
pub const RET_ERRNO: u32 = 0x0005_0000;
pub const RET_KILL_PROCESS: u32 = 0x8000_0000;

/// The flags of a new namespace that `clone`'s first argument may not
/// carry. (`CLONE_NEWTIME` is `clone3`'s and `unshare`'s alone: in `clone`
/// that bit is part of the exit signal.)
pub const CLONE_NEW_MASK: u32 = (libc::CLONE_NEWNS
    | libc::CLONE_NEWCGROUP
    | libc::CLONE_NEWUTS
    | libc::CLONE_NEWIPC
    | libc::CLONE_NEWUSER
    | libc::CLONE_NEWPID
    | libc::CLONE_NEWNET) as u32;

/// The denied system calls, each with its name: every one gets `EPERM`.
pub fn denied() -> Vec<(&'static str, libc::c_long)> {
    let mut v = vec![
        ("unshare", libc::SYS_unshare),
        ("setns", libc::SYS_setns),
        ("mount", libc::SYS_mount),
        ("umount2", libc::SYS_umount2),
        ("pivot_root", libc::SYS_pivot_root),
        ("open_tree", libc::SYS_open_tree),
        ("move_mount", libc::SYS_move_mount),
        ("fsopen", libc::SYS_fsopen),
        ("fsconfig", libc::SYS_fsconfig),
        ("fsmount", libc::SYS_fsmount),
        ("fspick", libc::SYS_fspick),
        ("mount_setattr", libc::SYS_mount_setattr),
        ("init_module", libc::SYS_init_module),
        ("finit_module", libc::SYS_finit_module),
        ("delete_module", libc::SYS_delete_module),
        ("kexec_load", libc::SYS_kexec_load),
        ("kexec_file_load", libc::SYS_kexec_file_load),
        ("reboot", libc::SYS_reboot),
        ("swapon", libc::SYS_swapon),
        ("swapoff", libc::SYS_swapoff),
        ("acct", libc::SYS_acct),
        ("quotactl", libc::SYS_quotactl),
        ("quotactl_fd", libc::SYS_quotactl_fd),
        ("settimeofday", libc::SYS_settimeofday),
        ("clock_settime", libc::SYS_clock_settime),
        ("clock_adjtime", libc::SYS_clock_adjtime),
        ("adjtimex", libc::SYS_adjtimex),
        ("ptrace", libc::SYS_ptrace),
        ("process_vm_readv", libc::SYS_process_vm_readv),
        ("process_vm_writev", libc::SYS_process_vm_writev),
        ("perf_event_open", libc::SYS_perf_event_open),
        ("bpf", libc::SYS_bpf),
        ("userfaultfd", libc::SYS_userfaultfd),
        ("open_by_handle_at", libc::SYS_open_by_handle_at),
        ("name_to_handle_at", libc::SYS_name_to_handle_at),
        ("kcmp", libc::SYS_kcmp),
        ("pidfd_getfd", libc::SYS_pidfd_getfd),
        ("lookup_dcookie", libc::SYS_lookup_dcookie),
        ("keyctl", libc::SYS_keyctl),
        ("add_key", libc::SYS_add_key),
        ("request_key", libc::SYS_request_key),
        ("io_uring_setup", libc::SYS_io_uring_setup),
        ("io_uring_enter", libc::SYS_io_uring_enter),
        ("io_uring_register", libc::SYS_io_uring_register),
    ];
    #[cfg(target_arch = "x86_64")]
    v.extend([
        ("iopl", libc::SYS_iopl),
        ("ioperm", libc::SYS_ioperm),
        ("uselib", libc::SYS_uselib),
    ]);
    v
}

/// Where a jump goes, resolved to an offset once the program is laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum To {
    Next,
    CloneFlags,
    Enosys,
    Eperm,
    Kill,
}

struct Insn {
    code: u16,
    k: u32,
    jt: To,
    jf: To,
}

const LD_W_ABS: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
const JEQ: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
const JGE: u16 = (libc::BPF_JMP | libc::BPF_JGE | libc::BPF_K) as u16;
const JSET: u16 = (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16;
const RET: u16 = (libc::BPF_RET | libc::BPF_K) as u16;

fn ld(off: u32) -> Insn {
    Insn {
        code: LD_W_ABS,
        k: off,
        jt: To::Next,
        jf: To::Next,
    }
}

fn jmp(code: u16, k: u32, jt: To, jf: To) -> Insn {
    Insn { code, k, jt, jf }
}

fn ret(k: u32) -> Insn {
    Insn {
        code: RET,
        k,
        jt: To::Next,
        jf: To::Next,
    }
}

/// Appends `insn`, and gives its index.
fn push(p: &mut Vec<Insn>, insn: Insn) -> usize {
    p.push(insn);
    p.len() - 1
}

/// The filter's program.
pub fn program() -> Vec<sock_filter> {
    let mut p = vec![
        ld(OFF_ARCH),
        jmp(JEQ, AUDIT_ARCH, To::Next, To::Kill),
        ld(OFF_NR),
    ];
    #[cfg(target_arch = "x86_64")]
    p.push(jmp(JGE, X32_SYSCALL_BIT, To::Kill, To::Next));
    p.push(jmp(JEQ, libc::SYS_clone as u32, To::CloneFlags, To::Next));
    p.push(jmp(JEQ, libc::SYS_clone3 as u32, To::Enosys, To::Next));
    for (_, nr) in denied() {
        p.push(jmp(JEQ, nr as u32, To::Eperm, To::Next));
    }
    p.push(ret(RET_ALLOW));
    // Classic BPF jumps forward only, so clone's check ends in an allow of
    // its own.
    let clone_flags = push(&mut p, ld(OFF_ARG0_LO));
    p.push(jmp(JSET, CLONE_NEW_MASK, To::Eperm, To::Next));
    p.push(ret(RET_ALLOW));
    let enosys = push(&mut p, ret(RET_ERRNO | libc::ENOSYS as u32));
    let eperm = push(&mut p, ret(RET_ERRNO | libc::EPERM as u32));
    let kill = push(&mut p, ret(RET_KILL_PROCESS));
    let resolve = |i: usize, to: To| -> u8 {
        let target = match to {
            To::Next => i + 1,
            To::CloneFlags => clone_flags,
            To::Enosys => enosys,
            To::Eperm => eperm,
            To::Kill => kill,
        };
        let ahead = target
            .checked_sub(i + 1)
            .expect("classic BPF jumps forward only");
        u8::try_from(ahead).expect("a BPF jump within 255 instructions")
    };
    p.iter()
        .enumerate()
        .map(|(i, insn)| sock_filter {
            code: insn.code,
            jt: resolve(i, insn.jt),
            jf: resolve(i, insn.jf),
            k: insn.k,
        })
        .collect()
}

/// Installs `program` on the calling thread. It needs `no_new_privs` (or
/// `CAP_SYS_ADMIN`), and lasts for the process and its children.
pub fn install(program: &[sock_filter]) -> io::Result<()> {
    let prog = libc::sock_fprog {
        len: program.len() as libc::c_ushort,
        filter: program.as_ptr() as *mut sock_filter,
    };
    crate::sys::cvt_long(unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            0,
            &prog as *const libc::sock_fprog,
        )
    })
    .map(drop)
}

/// A host that refuses some system calls, as a container's seccomp profile
/// does, stood in for a test (theseus-f7tz): on the calling thread, and every
/// process it starts after, each of `calls` answers its errno at once (a
/// `(number, errno)` pair: Docker's profile answers `clone3` with ENOSYS, and
/// an older one every call it does not know with EPERM), and anything else
/// runs. It sets `no_new_privs` first, as a filter without `CAP_SYS_ADMIN`
/// needs.
pub fn refuse_here(calls: &[(libc::c_long, i32)]) -> io::Result<()> {
    let mut prog = vec![sock_filter {
        code: LD_W_ABS,
        jt: 0,
        jf: 0,
        k: OFF_NR,
    }];
    for &(nr, errno) in calls {
        prog.push(sock_filter {
            code: JEQ,
            jt: 0,
            jf: 1,
            k: nr as u32,
        });
        prog.push(sock_filter {
            code: RET,
            jt: 0,
            jf: 0,
            k: RET_ERRNO | errno as u32,
        });
    }
    prog.push(sock_filter {
        code: RET,
        jt: 0,
        jf: 0,
        k: RET_ALLOW,
    });
    crate::sys::cvt(unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) })?;
    install(&prog)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a classic BPF program over one `seccomp_data`, as the kernel would
    /// (the subset the filter uses).
    fn run(prog: &[sock_filter], arch: u32, nr: u32, arg0: u64) -> u32 {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(&nr.to_ne_bytes());
        data[4..8].copy_from_slice(&arch.to_ne_bytes());
        data[16..24].copy_from_slice(&arg0.to_ne_bytes());
        let (mut a, mut pc) = (0u32, 0usize);
        loop {
            let i = prog[pc];
            let k = i.k as usize;
            let branch = |taken: bool| pc + 1 + usize::from(if taken { i.jt } else { i.jf });
            pc = match i.code {
                LD_W_ABS => {
                    a = u32::from_ne_bytes(data[k..k + 4].try_into().unwrap());
                    pc + 1
                }
                JEQ => branch(a == i.k),
                JGE => branch(a >= i.k),
                JSET => branch(a & i.k != 0),
                RET => return i.k,
                c => panic!("an instruction the filter never uses: {c:#x}"),
            };
        }
    }

    fn eperm() -> u32 {
        RET_ERRNO | libc::EPERM as u32
    }

    #[test]
    fn every_denied_call_gets_eperm_and_the_rest_are_allowed() {
        let p = program();
        for (name, nr) in denied() {
            assert_eq!(run(&p, AUDIT_ARCH, nr as u32, 0), eperm(), "{name}");
        }
        for (name, nr) in [
            ("read", libc::SYS_read),
            ("write", libc::SYS_write),
            ("openat", libc::SYS_openat),
            ("execve", libc::SYS_execve),
            ("wait4", libc::SYS_wait4),
            ("socket", libc::SYS_socket),
            ("connect", libc::SYS_connect),
            ("prctl", libc::SYS_prctl),
        ] {
            assert_eq!(run(&p, AUDIT_ARCH, nr as u32, 0), RET_ALLOW, "{name}");
        }
    }

    #[test]
    fn clone_is_judged_by_its_flags_and_clone3_falls_back() {
        let p = program();
        let clone = libc::SYS_clone as u32;
        let thread = (libc::CLONE_VM
            | libc::CLONE_FS
            | libc::CLONE_FILES
            | libc::CLONE_SIGHAND
            | libc::CLONE_THREAD
            | libc::CLONE_SYSVSEM
            | libc::CLONE_SETTLS
            | libc::CLONE_PARENT_SETTID
            | libc::CLONE_CHILD_CLEARTID) as u64;
        assert_eq!(run(&p, AUDIT_ARCH, clone, libc::SIGCHLD as u64), RET_ALLOW);
        assert_eq!(run(&p, AUDIT_ARCH, clone, thread), RET_ALLOW);
        for flag in [
            libc::CLONE_NEWNS,
            libc::CLONE_NEWCGROUP,
            libc::CLONE_NEWUTS,
            libc::CLONE_NEWIPC,
            libc::CLONE_NEWUSER,
            libc::CLONE_NEWPID,
            libc::CLONE_NEWNET,
        ] {
            let flags = (flag | libc::SIGCHLD) as u64;
            assert_eq!(run(&p, AUDIT_ARCH, clone, flags), eperm(), "{flag:#x}");
        }
        // Only the low 32 bits are clone's flags; a high bit changes nothing.
        assert_eq!(
            run(&p, AUDIT_ARCH, clone, (1 << 40) | libc::SIGCHLD as u64),
            RET_ALLOW
        );
        assert_eq!(
            run(&p, AUDIT_ARCH, libc::SYS_clone3 as u32, 0),
            RET_ERRNO | libc::ENOSYS as u32
        );
    }

    #[test]
    fn a_foreign_abi_is_killed() {
        let p = program();
        const AUDIT_ARCH_I386: u32 = 0x4000_0003;
        assert_eq!(run(&p, AUDIT_ARCH_I386, 20, 0), RET_KILL_PROCESS);
        #[cfg(target_arch = "x86_64")]
        assert_eq!(
            run(&p, AUDIT_ARCH, X32_SYSCALL_BIT | libc::SYS_getpid as u32, 0),
            RET_KILL_PROCESS
        );
    }

    #[test]
    fn the_program_is_small_and_every_jump_lands_inside_it() {
        let p = program();
        assert!(p.len() < 128, "{} instructions", p.len());
        for (i, insn) in p.iter().enumerate() {
            if insn.code != RET && insn.code != LD_W_ABS {
                assert!(i + 1 + usize::from(insn.jt) < p.len());
                assert!(i + 1 + usize::from(insn.jf) < p.len());
            }
        }
        assert_eq!(p.last().map(|i| i.code), Some(RET));
    }
}
