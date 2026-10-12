/*
 * The hand image's rootless build (rootless.py): an LD_PRELOAD shim for the chown family, so that the Dockerfile's
 * RUN step can run as root in a user namespace that maps one id. There a chown to any other id fails with EINVAL,
 * and dpkg treats that as fatal (openssh-client's postinst does `chgrp _ssh ssh-agent`). The shim records every
 * call as `<dev> <ino> <uid> <gid>` (an id of -1 left as it is) in the file THESEUS_OWNERS_LOG names, then makes the
 * real call; when that fails with EINVAL it answers 0. The layer writer gives each file the owner its inode was last
 * given, so the layer carries the ids a real root would have set. Keyed by inode, a record follows a rename
 * (dpkg's `.dpkg-new` files) and a replaced file does not inherit it.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

static void record(const struct stat *st, uid_t uid, gid_t gid) {
    const char *path = getenv("THESEUS_OWNERS_LOG");
    if (!path || !*path)
        return;
    char line[128];
    int n = snprintf(line, sizeof line, "%llu %llu %lld %lld\n", (unsigned long long)st->st_dev,
                     (unsigned long long)st->st_ino, (long long)(int)uid, (long long)(int)gid);
    int saved = errno;
    int fd = open(path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
    if (fd >= 0) {
        if (write(fd, line, (size_t)n) < 0) {
            /* a lost record shows as a root-owned file; nothing to do here */
        }
        close(fd);
    }
    errno = saved;
}

static int settle(int rc) {
    if (rc != 0 && errno == EINVAL) {
        errno = 0;
        return 0;
    }
    return rc;
}

typedef int (*path_fn)(const char *, uid_t, gid_t);
typedef int (*fd_fn)(int, uid_t, gid_t);
typedef int (*at_fn)(int, const char *, uid_t, gid_t, int);

#define REAL(name, type) \
    static type real_##name; \
    if (!real_##name) \
        real_##name = (type)dlsym(RTLD_NEXT, #name)

int chown(const char *path, uid_t uid, gid_t gid) {
    REAL(chown, path_fn);
    struct stat st;
    if (stat(path, &st) == 0)
        record(&st, uid, gid);
    return settle(real_chown(path, uid, gid));
}

int lchown(const char *path, uid_t uid, gid_t gid) {
    REAL(lchown, path_fn);
    struct stat st;
    if (lstat(path, &st) == 0)
        record(&st, uid, gid);
    return settle(real_lchown(path, uid, gid));
}

int fchown(int fd, uid_t uid, gid_t gid) {
    REAL(fchown, fd_fn);
    struct stat st;
    if (fstat(fd, &st) == 0)
        record(&st, uid, gid);
    return settle(real_fchown(fd, uid, gid));
}

int fchownat(int dirfd, const char *path, uid_t uid, gid_t gid, int flags) {
    REAL(fchownat, at_fn);
    struct stat st;
    if (fstatat(dirfd, path, &st, flags & (AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH)) == 0)
        record(&st, uid, gid);
    return settle(real_fchownat(dirfd, path, uid, gid, flags));
}
