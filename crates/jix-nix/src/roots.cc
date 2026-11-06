// Nix's C API does not expose permanent roots. Keep this small adapter pinned
// to the same upstream Nix as nix-bindings-rust; never spawn nix-store.
#include <nix/store/store-api.hh>
#include <nix/store/store-open.hh>
#include <nix/store/local-fs-store.hh>
#include <cstring>
#include <cstdlib>
extern "C" char *jix_root(const char *target, const char *root) noexcept {
    try {
        auto store = nix::openStore();
        auto local = dynamic_cast<nix::LocalFSStore *>(&*store);
        if (!local) return strdup("Jix requires a local filesystem Nix store");
        local->addPermRoot(store->parseStorePath(target), root);
        return nullptr;
    } catch (const std::exception &error) { return strdup(error.what()); }
    catch (...) { return strdup("Nix root registration failed"); }
}
extern "C" void jix_root_error_free(char *error) noexcept { free(error); }

// Offline evaluation may read the local daemon/cache, but cannot open Internet
// sockets or launch external fetch helpers. Installed before Nix starts workers.
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <linux/audit.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/socket.h>
#include <cerrno>
#include <cstddef>
extern "C" char *jix_offline() noexcept {
#if defined(__x86_64__)
    constexpr unsigned int arch = AUDIT_ARCH_X86_64;
#elif defined(__aarch64__)
    constexpr unsigned int arch = AUDIT_ARCH_AARCH64;
#else
    return strdup("Offline Nix evaluation requires a supported Linux architecture");
#endif
    sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, arch, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_execve, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_execveat, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_io_uring_setup, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_socket, 0, 4),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(seccomp_data, args[0])),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AF_UNIX, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | EPERM),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    };
    sock_fprog program{static_cast<unsigned short>(sizeof(filter)/sizeof(filter[0])), filter};
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &program))
        return strdup(strerror(errno));
    return nullptr;
}

#include <nix_api_expr_internal.h>
extern "C" void jix_configure_eval(nix_eval_state_builder *builder) noexcept {
    builder->readOnlyMode = false;
    builder->settings.set("allow-import-from-derivation", "false");
}
