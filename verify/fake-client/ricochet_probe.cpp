// Test-only Metamod plugin for a disposable stock Ricochet server.
#include <cstdio>
#include <cstring>
#include <extdll.h>
#include <meta_api.h>

static enginefuncs_t engine;
static meta_globals_t *meta;
static gamedll_funcs_t *game;
static edict_t *current;
static int phase, wait_frames, original_slot, original_serial, original_userid, disconnects;
static plugin_info_t info = {
    META_INTERFACE_VERSION, "SDK Ricochet fake-client probe", "1", "",
    "metamod-sdk-rs", "", "FAKEPROBE", PT_STARTUP, PT_NEVER
};

static void report(const char *status, const char *message) {
    std::printf("FAKE_STOCK %s %s\n", status, message);
    std::fflush(stdout);
}

static void disconnect(edict_t *entity) {
    meta->mres = MRES_IGNORED;
    if (entity == current && (phase == 1 || phase == 2)) {
        ++disconnects;
        report("EVENT", entity->pvPrivateData ? "disconnect with private data" : "disconnect without private data");
        if (!entity->pvPrivateData) phase = -1;
    }
}

static void frame() {
    meta->mres = MRES_IGNORED;
    if (phase < 0 || phase > 2) return;
    if (phase != 0 && disconnects < phase) {
        if (++wait_frames < 120) return;
        phase = -1;
        report("FAIL", "queued kick did not dispatch disconnect");
        return;
    }
    if (phase == 2) {
        phase = 3;
        report("PASS", "completed two setup and kick sequences");
        return;
    }
    edict_t *entity = engine.pfnCreateFakeClient(phase == 0 ? "SDKProbeFirst" : "SDKProbeSecond");
    if (!entity) {
        phase = -1;
        report("FAIL", "engine allocation returned null");
        return;
    }
    const bool allocated_as_fake = (entity->v.flags & FL_FAKECLIENT) != 0;
    char reason[128]{};
    // These are the original GameDLL functions. This test invokes them
    // explicitly; it does not expect Metamod admission or join hook delivery.
    if (!game->dllapi_table->pfnClientConnect(entity, phase == 0 ? "SDKProbeFirst" : "SDKProbeSecond",
                                            "loopback", reason)) {
        phase = -1;
        report("FAIL", "stock GameDLL rejected setup");
        return;
    }
    game->dllapi_table->pfnClientPutInServer(entity);
    const int slot = engine.pfnIndexOfEdict(entity);
    const int userid = engine.pfnGetPlayerUserId(entity);
    const bool spawned_as_fake = (entity->v.flags & FL_FAKECLIENT) != 0;
    std::printf("FAKE_STOCK STATE slot=%d userid=%d private=%d allocated_as_fake=%d spawned_as_fake=%d\n",
                slot, userid, entity->pvPrivateData != nullptr, allocated_as_fake, spawned_as_fake);
    if (!entity->pvPrivateData || !allocated_as_fake || userid <= 0) {
        phase = -1;
        report("FAIL", "setup did not produce a fake client with private data and a user ID");
        return;
    }
    // Stock Ricochet Spawn assigns FL_CLIENT and can clear FL_FAKECLIENT.
    // Restore the marker for this engine-allocated fake client before play.
    entity->v.flags |= FL_FAKECLIENT;
    if (phase == 0) {
        original_slot = slot;
        original_serial = entity->serialnumber;
        original_userid = userid;
    } else if (slot != original_slot || userid == original_userid) {
        phase = -1;
        report("FAIL", "replacement did not reuse the slot with a distinct user ID");
        return;
    }
    std::printf("FAKE_STOCK SETUP slot=%d userid=%d serial=%d same_edict=%d same_serial=%d\n",
                slot, userid, entity->serialnumber, entity == current, entity->serialnumber == original_serial);
    std::fflush(stdout);
    current = entity;
    ++phase;
    wait_frames = 0;
    char command[48];
    std::snprintf(command, sizeof command, "kick #%d\n", userid);
    // Queue for normal host command execution. Do not nest ServerExecute.
    engine.pfnServerCommand(command);
}

static int hooks(DLL_FUNCTIONS *out, int *version) {
    if (*version != INTERFACE_VERSION) {
        *version = INTERFACE_VERSION;
        return 0;
    }
    *out = {};
    out->pfnStartFrame = frame;
    out->pfnClientDisconnect = disconnect;
    return 1;
}

extern "C" void GiveFnptrsToDll(enginefuncs_t *functions, globalvars_t *) { engine = *functions; }
C_DLLEXPORT int Meta_Query(const char *version, plugin_info_t **out, mutil_funcs_t *) {
    if (std::strcmp(version, META_INTERFACE_VERSION) != 0) return 0;
    *out = &info;
    return 1;
}
C_DLLEXPORT int Meta_Attach(PLUG_LOADTIME now, META_FUNCTIONS *out, meta_globals_t *globals, gamedll_funcs_t *dll) {
    if (now != PT_STARTUP) return 0;
    *out = {};
    out->pfnGetEntityAPI2 = hooks;
    meta = globals;
    game = dll;
    return 1;
}
C_DLLEXPORT int Meta_Detach(PLUG_LOADTIME, PL_UNLOAD_REASON) { return 0; }
