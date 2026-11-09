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
//! - the kernel: modules, `kexec_*`, `reboot`, swap, `acct`, quotas, the
//!   time setters, and the kernel log (`syslog`: a user namespace does not
//!   virtualize the ring buffer, and where `kernel.dmesg_restrict` is 0 the
//!   read needs no capability, so the filter is the whole of the boundary);
//! - introspection: `ptrace`, `process_vm_*`, `perf_event_open`, `bpf`,
//!   `userfaultfd`, the file-handle calls, `kcmp`, `pidfd_getfd`, and (x86)
//!   `iopl` and `ioperm`;
//! - keys: `keyctl`, `add_key`, and `request_key`;
//! - `io_uring_*`, whose operations would bypass the filter.
//!
//! A foreign ABI (i386 or x32 on x86_64, 32-bit Arm on aarch64) kills the
//! process. The numbers are each architecture's own (`libc::SYS_*`), and the
//! x86-only calls and the x32 check are compiled for x86_64 alone.

use std::io;

use libc::sock_filter;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // EM_X86_64 | 64-bit | little-endian
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7; // EM_AARCH64 | 64-bit | little-endian
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("L1's seccomp filter has tables for x86_64 and aarch64 only");

/// aarch64's `kexec_file_load`, the generic table's number: the libc crate
/// leaves it out for aarch64 musl.
#[cfg(all(target_arch = "aarch64", any(target_env = "musl", test)))]
const AARCH64_KEXEC_FILE_LOAD: libc::c_long = 294;
#[cfg(all(target_arch = "aarch64", target_env = "musl"))]
const SYS_KEXEC_FILE_LOAD: libc::c_long = AARCH64_KEXEC_FILE_LOAD;
#[cfg(not(all(target_arch = "aarch64", target_env = "musl")))]
const SYS_KEXEC_FILE_LOAD: libc::c_long = libc::SYS_kexec_file_load;

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
///
/// **The parity rule, and the asymmetry nobody had written down**
/// (theseus-3i6d1). Docker's default seccomp profile is an **allowlist**: a
/// system call it does not name is blocked. This filter **allows by
/// default** and denies what is listed here, so *everything Docker blocks by
/// omission we ALLOW unless someone typed it*. The shape of the list follows
/// Docker's profile (the module doc above groups it the same way), but the
/// consequence of a gap is the opposite of Docker's: there, a call nobody
/// thought about is refused; here, it runs.
///
/// So this list is a list that must be **complete**, and completeness is
/// what `tests::MUST_DENY` holds it to. That table is the requirement,
/// written apart from this list and grouped by the reason each call is
/// refused, and `the_filter_denies_every_call_the_parity_rule_requires`
/// **runs the built filter** over every one of its entries rather than
/// reading this list: a call taken out of `denied()` is then a filter that
/// allows it, and the test says so by name. Adding a call here means adding
/// it there, with its reason, and the failure names both places.
pub fn denied() -> Vec<(&'static str, libc::c_long)> {
    let v = vec![
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
        ("kexec_file_load", SYS_KEXEC_FILE_LOAD),
        ("reboot", libc::SYS_reboot),
        ("swapon", libc::SYS_swapon),
        ("swapoff", libc::SYS_swapoff),
        ("acct", libc::SYS_acct),
        ("syslog", libc::SYS_syslog),
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
    // x86_64 alone has these (aarch64 has no port I/O and no `uselib`).
    #[cfg(target_arch = "x86_64")]
    let v = [
        v,
        vec![
            ("iopl", libc::SYS_iopl),
            ("ioperm", libc::SYS_ioperm),
            ("uselib", libc::SYS_uselib),
        ],
    ]
    .concat();
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
// The x32 check alone uses it (and the tests' interpreter).
#[cfg(any(target_arch = "x86_64", test))]
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

    #[cfg(all(target_arch = "aarch64", target_env = "gnu"))]
    #[test]
    fn aarch64_musls_kexec_file_load_number_is_glibcs() {
        assert_eq!(AARCH64_KEXEC_FILE_LOAD, libc::SYS_kexec_file_load);
    }

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

    /// theseus-8f50c. `every_denied_call_gets_eperm_and_the_rest_are_allowed`
    /// reads the list it is judging, so it says nothing about a call the list
    /// leaves out. The kernel's ring buffer is one the list must hold by name:
    /// `syslog(SYSLOG_ACTION_READ_ALL)` needs no capability where
    /// `kernel.dmesg_restrict` is 0, and a user namespace does not virtualize
    /// it, so the filter is the whole of the boundary. This holds on both
    /// architectures, whatever the running host's `dmesg_restrict` says.
    #[test]
    fn the_filter_denies_the_kernel_logs_calls() {
        let p = program();
        assert_eq!(
            run(&p, AUDIT_ARCH, libc::SYS_syslog as u32, 0),
            eperm(),
            "syslog"
        );
        assert!(
            denied()
                .iter()
                .any(|&(n, nr)| n == "syslog" && nr == libc::SYS_syslog),
            "syslog is not in the denied list by name"
        );
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
        const AUDIT_ARCH_ARM: u32 = 0x4000_0028; // 32-bit Arm, aarch64's compat ABI
        assert_eq!(run(&p, AUDIT_ARCH_I386, 20, 0), RET_KILL_PROCESS);
        assert_eq!(run(&p, AUDIT_ARCH_ARM, 20, 0), RET_KILL_PROCESS);
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

    /// **The requirement**, written apart from `denied()` (theseus-3i6d1).
    ///
    /// `every_denied_call_gets_eperm_and_the_rest_are_allowed` walks
    /// `denied()` and asks the filter about each entry, so it reads the very
    /// list it is judging and says nothing at all about a call the list
    /// leaves out. The reviewer proved it: deleting `perf_event_open` from
    /// `denied()` left the whole sandbox suite 40/40 green, and the live
    /// contract names 10 of the list's 45 entries, so 35 denied calls were
    /// asserted by nothing and any one of them could be dropped in silence.
    ///
    /// This is the other document. Each row is `(why it is refused, its
    /// name, its number)`, grouped by the reason rather than by the order
    /// `denied()` happens to build its vector in. **The two hold the same 44
    /// entries in the same order, with the reasons added (and x86_64's three
    /// more, `iopl`, `ioperm` and `uselib`, are in `MUST_DENY_X86` beside
    /// their own rows in `denied()`): they are one text copied twice.** `the_filter_denies_every_call_the_parity_rule_requires`
    /// runs the **built filter** over every row — it never reads `denied()`
    /// to decide what to ask — and then holds the two documents to the same
    /// set in both directions.
    ///
    /// What that catches and what it does not (theseus-3i6d1): a deletion
    /// from **one** table is caught, because the filter is run over the rows
    /// and the two are compared. A deletion from **both**, six lines apart in
    /// this one file, is not: nothing else says the call should be refused.
    /// And a system call on **neither** table is enumerated by nobody: the
    /// filter allows by default, so a call no one thought of runs. That last
    /// gap is the real residual, and no test here closes it.
    ///
    /// A call added here that the filter does not refuse fails. A call
    /// `denied()` holds that is not here fails too: a denial with no written
    /// reason is a denial nobody can review.
    const MUST_DENY: &[(&str, &str, libc::c_long)] = &[
        // A job has its own mount namespace and its view is built for it; a
        // program that can make or move mounts can take itself out of that
        // view, and `setns`/`unshare` leave the namespaces it was given.
        ("namespaces and mounts", "unshare", libc::SYS_unshare),
        ("namespaces and mounts", "setns", libc::SYS_setns),
        ("namespaces and mounts", "mount", libc::SYS_mount),
        ("namespaces and mounts", "umount2", libc::SYS_umount2),
        ("namespaces and mounts", "pivot_root", libc::SYS_pivot_root),
        // The new mount API reaches the same place by another door.
        ("namespaces and mounts", "open_tree", libc::SYS_open_tree),
        ("namespaces and mounts", "move_mount", libc::SYS_move_mount),
        ("namespaces and mounts", "fsopen", libc::SYS_fsopen),
        ("namespaces and mounts", "fsconfig", libc::SYS_fsconfig),
        ("namespaces and mounts", "fsmount", libc::SYS_fsmount),
        ("namespaces and mounts", "fspick", libc::SYS_fspick),
        (
            "namespaces and mounts",
            "mount_setattr",
            libc::SYS_mount_setattr,
        ),
        // Code loaded into the kernel is outside every boundary above it.
        ("kernel modules", "init_module", libc::SYS_init_module),
        ("kernel modules", "finit_module", libc::SYS_finit_module),
        ("kernel modules", "delete_module", libc::SYS_delete_module),
        // Replacing or restarting the running kernel.
        ("the kernel's own life", "kexec_load", libc::SYS_kexec_load),
        (
            "the kernel's own life",
            "kexec_file_load",
            SYS_KEXEC_FILE_LOAD,
        ),
        ("the kernel's own life", "reboot", libc::SYS_reboot),
        // Host-wide state a job has no business setting.
        ("swap and accounting", "swapon", libc::SYS_swapon),
        ("swap and accounting", "swapoff", libc::SYS_swapoff),
        ("swap and accounting", "acct", libc::SYS_acct),
        // A user namespace does not virtualize the ring buffer, and where
        // `kernel.dmesg_restrict` is 0 the read needs no capability, so the
        // filter is the whole of the boundary (theseus-8f50c).
        ("the kernel log", "syslog", libc::SYS_syslog),
        ("quotas", "quotactl", libc::SYS_quotactl),
        ("quotas", "quotactl_fd", libc::SYS_quotactl_fd),
        // The host's clock is shared; moving it moves every deadline on it.
        ("the clock", "settimeofday", libc::SYS_settimeofday),
        ("the clock", "clock_settime", libc::SYS_clock_settime),
        ("the clock", "clock_adjtime", libc::SYS_clock_adjtime),
        ("the clock", "adjtimex", libc::SYS_adjtimex),
        // Reading or steering another process, or the machine underneath.
        ("introspection", "ptrace", libc::SYS_ptrace),
        (
            "introspection",
            "process_vm_readv",
            libc::SYS_process_vm_readv,
        ),
        (
            "introspection",
            "process_vm_writev",
            libc::SYS_process_vm_writev,
        ),
        (
            "introspection",
            "perf_event_open",
            libc::SYS_perf_event_open,
        ),
        ("introspection", "bpf", libc::SYS_bpf),
        ("introspection", "userfaultfd", libc::SYS_userfaultfd),
        // A file handle names a file outside the view the job was given.
        (
            "introspection",
            "open_by_handle_at",
            libc::SYS_open_by_handle_at,
        ),
        (
            "introspection",
            "name_to_handle_at",
            libc::SYS_name_to_handle_at,
        ),
        ("introspection", "kcmp", libc::SYS_kcmp),
        ("introspection", "pidfd_getfd", libc::SYS_pidfd_getfd),
        ("introspection", "lookup_dcookie", libc::SYS_lookup_dcookie),
        // The kernel keyring is shared with the host's own keys.
        ("the kernel keyring", "keyctl", libc::SYS_keyctl),
        ("the kernel keyring", "add_key", libc::SYS_add_key),
        ("the kernel keyring", "request_key", libc::SYS_request_key),
        // io_uring's operations run in the kernel and would bypass the
        // filter entirely: the ring is submitted once and acts many times.
        ("io_uring", "io_uring_setup", libc::SYS_io_uring_setup),
        ("io_uring", "io_uring_enter", libc::SYS_io_uring_enter),
        ("io_uring", "io_uring_register", libc::SYS_io_uring_register),
    ];

    /// x86_64's own: port I/O, and the ancient shared-library loader.
    /// aarch64 has neither (`libc` does not define them there).
    #[cfg(target_arch = "x86_64")]
    const MUST_DENY_X86: &[(&str, &str, libc::c_long)] = &[
        ("port I/O", "iopl", libc::SYS_iopl),
        ("port I/O", "ioperm", libc::SYS_ioperm),
        ("the old loader", "uselib", libc::SYS_uselib),
    ];

    /// The requirement for the architecture this build is for.
    fn must_deny() -> Vec<(&'static str, &'static str, libc::c_long)> {
        let v = MUST_DENY.to_vec();
        #[cfg(target_arch = "x86_64")]
        let v = [v, MUST_DENY_X86.to_vec()].concat();
        v
    }

    /// The completeness test (theseus-3i6d1), in the shape of
    /// `crates/complete.rs`: the **requirement** is walked, the filter is
    /// **run** for every row of it, and `denied()` is judged against that
    /// walk rather than being the thing that decides what to ask.
    ///
    /// The planted deletion the reviewer used is what this is measured by:
    /// take `perf_event_open` out of `denied()` and this goes red naming it
    /// and the filter that now allows it.
    #[test]
    fn the_filter_denies_every_call_the_parity_rule_requires() {
        let p = program();
        let rule = must_deny();
        // The positive bar first: a walk of nothing must not pass (the whole
        // bug class here is a check that judges an empty set). 45 is the
        // portable requirement; x86_64 adds three.
        assert!(
            rule.len() >= 45,
            "the parity rule names only {} system calls: MUST_DENY has been \
             emptied, and a filter judged against an empty requirement is no \
             filter at all",
            rule.len()
        );

        // Exercise, do not read: run the built program for every required
        // call and collect the ones it lets through.
        let mut allowed = Vec::new();
        for (why, name, nr) in &rule {
            let got = run(&p, AUDIT_ARCH, *nr as u32, 0);
            if got != eperm() {
                allowed.push(format!(
                    "  {name} (#{nr}, {why}) -> {got:#x}, not EPERM: the built \
                     filter lets it through"
                ));
            }
        }
        assert!(
            allowed.is_empty(),
            "{} of the {} system calls the parity rule requires are NOT denied \
             by the filter seccomp::program() builds. Either they were taken \
             out of seccomp::denied() (put them back), or the rule changed and \
             MUST_DENY in this file must say so:\n{}",
            allowed.len(),
            rule.len(),
            allowed.join("\n")
        );

        // And the two documents name the same set, in both directions, so a
        // denial with no written reason fails as loudly as a missing one.
        let spec = crate::complete::MustBeComplete {
            item: "a system call L1 must refuse",
            list: "theseus_sandbox::seccomp::denied()",
            world: "seccomp::tests::MUST_DENY, the parity rule",
            add: "add it to denied(), beside the call it belongs with",
            drop: "add it to MUST_DENY with the reason it is refused, or, if it \
                   is no longer required, take it out of denied() in the same commit",
            least: 45,
            stale_fails: true,
        };
        let walked = crate::complete::check(
            &spec,
            denied()
                .into_iter()
                .map(|(n, nr)| (n.to_string(), format!("#{nr}"))),
            rule.iter()
                .map(|(why, n, nr)| ((*n).to_string(), format!("#{nr}, {why}"))),
        );
        assert_eq!(walked, rule.len());
    }

    /// The test above is the one that catches a planted deletion, so its own
    /// walk must not be able to go quiet (theseus-3i6d1): `must_deny` reads
    /// nothing of `denied()`, and the two tables agree on every number.
    #[test]
    fn the_parity_rule_is_its_own_document_and_agrees_on_every_number() {
        let rule = must_deny();
        let list = denied();
        // The numbers first, by name, so the failure names the call rather
        // than two lengths.
        for (why, name, nr) in &rule {
            let found = list.iter().find(|(n, _)| n == name);
            let (_, listed) = found.unwrap_or_else(|| {
                panic!(
                    "seccomp::denied() has no entry for {name} ({why}), which the \
                     parity rule in MUST_DENY requires. Put it back in denied(), \
                     or, if the requirement really changed, take its MUST_DENY \
                     row out in the same commit and say why."
                )
            });
            assert_eq!(
                listed, nr,
                "{name} is #{listed} in denied() and #{nr} in MUST_DENY: one of \
                 the two names the wrong system call, and the filter is built \
                 from denied()"
            );
        }
        assert_eq!(
            rule.len(),
            list.len(),
            "the parity rule names {} system calls and denied() {}, and every \
             rule row was found in denied() by name: denied() holds a row the \
             rule does not, or a duplicate",
            rule.len(),
            list.len()
        );
    }
}
