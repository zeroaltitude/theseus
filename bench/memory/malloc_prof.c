/* malloc_prof.c: a sampling heap profiler for a glibc process, by LD_PRELOAD
 * (theseus-mem-daemon-import). It answers one question a resident-memory figure
 * cannot: of the bytes the process holds LIVE right now, which call stack allocated
 * each, and (through malloc_info) how many bytes glibc holds FREE in pieces it
 * cannot hand back.
 *
 *   cc -O2 -shared -fPIC -o malloc_prof.so malloc_prof.c -ldl -lpthread
 *   MALLOC_PROF_TRIGGER=/tmp/p.trigger MALLOC_PROF_OUT=/tmp/p LD_PRELOAD=./malloc_prof.so theseusd ...
 *   touch /tmp/p.trigger     # within 200 ms the profile is written to /tmp/p.<n>.txt and /tmp/p.<n>.info
 *
 * It samples by bytes (mean MALLOC_PROF_SAMPLE, default 4096, like jemalloc's
 * lg_prof_sample): an allocation of s bytes is sampled with probability
 * 1 - exp(-s/mean), and counts for s / that, so the live total is an unbiased estimate.
 * Each sample's stack is kept until its block is freed (a realloc frees and allocates).
 * What it cannot see: memory the process takes by mmap itself, the allocator's own
 * free lists (that is what the .info file is for), and blocks allocated before it loaded.
 * Stacks are raw addresses (module and offset); bench/memory/heap_owners.py names them.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <execinfo.h>
#include <malloc.h>
#include <math.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

extern void *__libc_malloc(size_t);
extern void __libc_free(void *);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void *__libc_memalign(size_t, size_t);

#define DEPTH 28
#define TABLE (1u << 21)
#define STACKS (1u << 16)
#define MARKS (1u << 25)

struct entry {
    void *ptr;
    uint32_t stack;
    uint32_t size;
    double weight;
};
struct stack {
    void *frames[DEPTH];
    int n;
    uint64_t hash;
    double live_bytes, live_count, total_bytes;
};

static struct entry *table;
static struct stack *stacks;
static uint8_t *marks;
static uint32_t nstacks;
static atomic_flag lock = ATOMIC_FLAG_INIT;
static double mean = 4096;
static int ready;
static __thread int busy;
static __thread long long until;
static __thread uint64_t rng;

static void take(void) { while (atomic_flag_test_and_set_explicit(&lock, memory_order_acquire)) { } }
static void drop(void) { atomic_flag_clear_explicit(&lock, memory_order_release); }

static uint64_t mix(uint64_t x) {
    x ^= x >> 33; x *= 0xff51afd7ed558ccdULL; x ^= x >> 33; x *= 0xc4ceb9fe1a85ec53ULL; x ^= x >> 33;
    return x;
}
static double uniform(void) {
    if (!rng) rng = mix((uint64_t)(uintptr_t)&rng ^ (uint64_t)pthread_self());
    rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
    return ((rng >> 11) + 1) / 9007199254740994.0;
}
static long long next_gap(void) { return (long long)(-log(uniform()) * mean) + 1; }

static uint32_t stack_id(void **fr, int n) {
    uint64_t h = 1469598103934665603ULL;
    for (int i = 0; i < n; i++) h = (h ^ (uint64_t)(uintptr_t)fr[i]) * 1099511628211ULL;
    uint32_t i = (uint32_t)(h % STACKS);
    for (uint32_t k = 0; k < STACKS; k++, i = (i + 1) % STACKS) {
        if (stacks[i].n == 0) {
            memcpy(stacks[i].frames, fr, (size_t)n * sizeof(void *));
            stacks[i].n = n; stacks[i].hash = h; nstacks++;
            return i;
        }
        if (stacks[i].hash == h && stacks[i].n == n) return i;
    }
    return 0;
}

static void sampled(void *p, size_t size) {
    if (!p || !ready || busy) return;
    until -= (long long)size;
    if (until > 0) return;
    until = next_gap();
    busy = 1;
    void *fr[DEPTH + 3];
    int n = backtrace(fr, DEPTH + 3);
    int skip = n > 3 ? 3 : 0;
    double prob = 1.0 - exp(-(double)size / mean);
    double weight = prob > 0 ? (double)size / prob : (double)size;
    take();
    uint32_t id = stack_id(fr + skip, n - skip);
    uint32_t h = (uint32_t)(mix((uint64_t)(uintptr_t)p) % TABLE);
    for (uint32_t k = 0; k < TABLE; k++, h = (h + 1) % TABLE) {
        if (!table[h].ptr || table[h].ptr == p) {
            table[h].ptr = p; table[h].stack = id; table[h].size = (uint32_t)(size > 0xffffffffu ? 0xffffffffu : size);
            table[h].weight = weight;
            break;
        }
    }
    stacks[id].live_bytes += weight; stacks[id].live_count += weight / (double)(size ? size : 1);
    stacks[id].total_bytes += weight;
    drop();
    marks[mix((uint64_t)(uintptr_t)p) % MARKS] = 1;
    busy = 0;
}

static void forget(void *p) {
    if (!p || !ready) return;
    if (!marks[mix((uint64_t)(uintptr_t)p) % MARKS]) return;
    take();
    uint32_t h = (uint32_t)(mix((uint64_t)(uintptr_t)p) % TABLE);
    for (uint32_t k = 0; k < TABLE; k++, h = (h + 1) % TABLE) {
        if (!table[h].ptr) break;
        if (table[h].ptr == p) {
            struct stack *s = &stacks[table[h].stack];
            s->live_bytes -= table[h].weight;
            s->live_count -= table[h].weight / (double)(table[h].size ? table[h].size : 1);
            /* Backward-shift deletion keeps probe chains whole. */
            uint32_t i = h, j = h;
            for (;;) {
                table[i].ptr = NULL;
                for (;;) {
                    j = (j + 1) % TABLE;
                    if (!table[j].ptr) goto done;
                    uint32_t k2 = (uint32_t)(mix((uint64_t)(uintptr_t)table[j].ptr) % TABLE);
                    if (i <= j ? (i < k2 && k2 <= j) : (i < k2 || k2 <= j)) continue;
                    break;
                }
                table[i] = table[j];
                i = j;
            }
        }
    }
done:
    drop();
}

void *malloc(size_t n) { void *p = __libc_malloc(n); sampled(p, n); return p; }
void *calloc(size_t a, size_t b) { void *p = __libc_calloc(a, b); sampled(p, a * b); return p; }
void free(void *p) { forget(p); __libc_free(p); }
void *realloc(void *p, size_t n) {
    forget(p);
    void *q = __libc_realloc(p, n);
    sampled(q, n);
    return q;
}
void *memalign(size_t a, size_t n) { void *p = __libc_memalign(a, n); sampled(p, n); return p; }
void *aligned_alloc(size_t a, size_t n) { void *p = __libc_memalign(a, n); sampled(p, n); return p; }
int posix_memalign(void **out, size_t a, size_t n) {
    void *p = __libc_memalign(a, n);
    if (!p) return ENOMEM;
    *out = p; sampled(p, n);
    return 0;
}

static void dump(int seq) {
    const char *base = getenv("MALLOC_PROF_OUT");
    if (!base) base = "/tmp/malloc_prof";
    char path[512];
    busy = 1;
    snprintf(path, sizeof path, "%s.%d.info", base, seq);
    FILE *f = fopen(path, "w");
    if (f) { malloc_info(0, f); fclose(f); }
    snprintf(path, sizeof path, "%s.%d.tmp", base, seq);
    f = fopen(path, "w");
    if (!f) { busy = 0; return; }
    take();
    for (uint32_t i = 0; i < STACKS; i++) {
        if (!stacks[i].n || stacks[i].live_bytes < 1) continue;
        fprintf(f, "%.0f %.0f %.0f |", stacks[i].live_bytes, stacks[i].live_count, stacks[i].total_bytes);
        for (int k = 0; k < stacks[i].n; k++) {
            Dl_info d;
            if (dladdr(stacks[i].frames[k], &d) && d.dli_fname)
                fprintf(f, " %s+0x%lx", d.dli_fname, (unsigned long)((char *)stacks[i].frames[k] - (char *)d.dli_fbase));
            else
                fprintf(f, " ?+%p", stacks[i].frames[k]);
        }
        fputc('\n', f);
    }
    drop();
    fclose(f);
    char to[512];
    snprintf(to, sizeof to, "%s.%d.txt", base, seq);
    rename(path, to);
    busy = 0;
}

static void *watch(void *arg) {
    (void)arg;
    const char *trig = getenv("MALLOC_PROF_TRIGGER");
    int seq = 0;
    busy = 1; /* this thread's own allocations are not the process's */
    for (;;) {
        usleep(200000);
        if (trig && access(trig, F_OK) == 0) {
            unlink(trig);
            dump(seq++);
            busy = 1;
        }
    }
    return NULL;
}

__attribute__((constructor)) static void init(void) {
    const char *s = getenv("MALLOC_PROF_SAMPLE");
    if (s && atof(s) > 0) mean = atof(s);
    /* Its own tables, by mmap: they are never the process's allocations. */
    int flags = PROT_READ | PROT_WRITE;
    table = mmap(NULL, sizeof(struct entry) * TABLE, flags, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    stacks = mmap(NULL, sizeof(struct stack) * STACKS, flags, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    marks = mmap(NULL, MARKS, flags, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (table == MAP_FAILED || stacks == MAP_FAILED || marks == MAP_FAILED) return;
    void *warm[4];
    busy = 1;
    backtrace(warm, 4); /* the unwinder allocates on first use */
    busy = 0;
    until = next_gap();
    pthread_t t;
    pthread_create(&t, NULL, watch, NULL);
    pthread_detach(t);
    ready = 1;
}
