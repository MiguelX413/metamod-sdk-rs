// Metamod observer for disposable user-message probes. No clients are connected.
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <extdll.h>
#include <meta_api.h>

static enginefuncs_t engine;
static meta_globals_t *meta;
static const char *mode;
static bool entered;
static int message_id;
static unsigned write_hook_calls;
static plugin_info_t info = {
    META_INTERFACE_VERSION, "SDK user-message reentry probe", "1", "",
    "metamod-sdk-rs", "", "MSGPROBE", PT_STARTUP, PT_NEVER
};

static void mark(const char *label) {
    std::printf("MESSAGE_HOOK %s\n", label);
    std::fflush(stdout);
}

static void write_byte(int value) {
    ++write_hook_calls;
    meta->mres = MRES_IGNORED;
    if (entered || value != 17 || std::strcmp(mode, "post-end") == 0) return;
    entered = true;
    mark(mode);
    if (std::strcmp(mode, "nested-begin") == 0) {
        engine.pfnMessageBegin(MSG_ALL, message_id, nullptr, nullptr);
        mark("unexpected nested begin return");
    } else if (std::strcmp(mode, "nested-write") == 0) {
        const unsigned before = write_hook_calls;
        engine.pfnWriteByte(23);
        mark("nested write returned");
        // Plugin engine-table calls bypass classic Metamod's GameDLL hooks.
        // Count actual hook entries instead of inferring bypass from a guard.
        mark(write_hook_calls == before
            ? "PASS plugin write bypassed observer"
            : "FAIL plugin write reentered observer");
    } else if (std::strcmp(mode, "nested-end") == 0) {
        engine.pfnWriteByte(23);
        engine.pfnMessageEnd();
        mark("nested end returned");
    }
    meta->mres = MRES_IGNORED;
}

static void end_post() {
    meta->mres = MRES_IGNORED;
    if (entered || std::strcmp(mode, "post-end") != 0) return;
    entered = true;
    mark("post-end");
    engine.pfnMessageBegin(MSG_ALL, message_id, nullptr, nullptr);
    engine.pfnWriteByte(23);
    engine.pfnMessageEnd();
    mark("second message completed");
    meta->mres = MRES_IGNORED;
}

// Learn the fixture's registration through the actual Metamod wrapper.
static int registered(const char *name, int) {
    meta->mres = MRES_IGNORED;
    if (std::strcmp(name, "SDKNested") == 0 && meta->orig_ret) {
        message_id = *static_cast<const int *>(meta->orig_ret);
    }
    return 0;
}

static int pre(enginefuncs_t *out, int *version) {
    if (*version != ENGINE_INTERFACE_VERSION) {
        *version = ENGINE_INTERFACE_VERSION;
        return 0;
    }
    *out = {};
    out->pfnWriteByte = write_byte;
    return 1;
}

static int post(enginefuncs_t *out, int *version) {
    if (*version != ENGINE_INTERFACE_VERSION) {
        *version = ENGINE_INTERFACE_VERSION;
        return 0;
    }
    *out = {};
    out->pfnRegUserMsg = registered;
    out->pfnMessageEnd = end_post;
    return 1;
}

extern "C" void GiveFnptrsToDll(enginefuncs_t *functions, globalvars_t *) { engine = *functions; }
C_DLLEXPORT int Meta_Query(const char *version, plugin_info_t **out, mutil_funcs_t *) {
    if (std::strcmp(version, META_INTERFACE_VERSION) != 0) return 0;
    *out = &info;
    return 1;
}
C_DLLEXPORT int Meta_Attach(PLUG_LOADTIME now, META_FUNCTIONS *out, meta_globals_t *globals, gamedll_funcs_t *) {
    mode = std::getenv("METAMOD_MESSAGE_PROBE");
    if (now != PT_STARTUP || !mode) return 0;
    *out = {};
    out->pfnGetEngineFunctions = pre;
    out->pfnGetEngineFunctions_Post = post;
    meta = globals;
    return 1;
}
C_DLLEXPORT int Meta_Detach(PLUG_LOADTIME, PL_UNLOAD_REASON) { return 0; }
