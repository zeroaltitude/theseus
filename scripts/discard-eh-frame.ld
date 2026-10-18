/* Drops the unwind tables from a release link (scripts/build.sh; theseus-rwpbg.5). `-C force-unwind-tables=no` only
   reaches the crates rustc compiles, and the precompiled std and C code still bring their .eh_frame. The profiles abort
   on panic, so nothing unwinds. An implicit linker script: the linker adds this to its own, and INSERT places it. */
SECTIONS { /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) } }
INSERT AFTER .text;
