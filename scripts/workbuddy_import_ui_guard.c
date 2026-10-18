/* QA-only process bootstrap. Product binaries remain unchanged. */
#include <dlfcn.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

typedef int (*apply_policy_fn)(const char *, uint64_t, char **);

__attribute__((constructor)) static void protect_source_profiles(void) {
    const char *path = getenv("WORKBUDDY_PROXY_QA_POLICY");
    const char *proof = getenv("WORKBUDDY_PROXY_QA_PROOF_DIR");
    if (!path || !proof) _exit(78);
    int fd = open(path, O_RDONLY);
    if (fd < 0) _exit(78);
    char policy[8192];
    ssize_t count = read(fd, policy, sizeof(policy) - 1);
    close(fd);
    if (count <= 0 || count == (ssize_t)sizeof(policy) - 1) _exit(78);
    policy[count] = 0;
    void *library = dlopen("/usr/lib/libsandbox.1.dylib", RTLD_NOW | RTLD_LOCAL);
    if (!library) _exit(78);
    apply_policy_fn apply = (apply_policy_fn)dlsym(library, "sandbox_init");
    char *error = NULL;
    if (!apply || apply(policy, 0, &error) != 0) _exit(78);
    char marker[PATH_MAX];
    int length = snprintf(marker, sizeof(marker), "%s/%d.enforced", proof, getpid());
    if (length < 0 || length >= (int)sizeof(marker)) _exit(78);
    fd = open(marker, O_CREAT | O_EXCL | O_WRONLY, 0600);
    if (fd < 0 || write(fd, "enforced\n", 9) != 9) _exit(78);
    close(fd);
}
