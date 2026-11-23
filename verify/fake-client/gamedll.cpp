// Dedicated, disposable test GameDLL. No real clients or gameplay.
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <extdll.h>

static enginefuncs_t engine;
static globalvars_t *globals;
static bool attempted;
static bool active;
static bool allocation_reentry;
static bool message_probe;
static bool change_requested;
static int map_activations;
static edict_t *prepared;
static edict_t *last_userinfo;
static edict_t *last_disconnected;
static int userinfo_calls, connect_calls, put_calls, disconnect_calls;

template<class T> struct Stub;
template<class R, class... A> struct Stub<R (*)(A...)> {
    static R call(A...) { return R(); }
};

static void log_client(const char *event, edict_t *entity) {
    std::printf("FAKE_FIXTURE %s slot=%d serial=%d prepared=%d\n", event,
                engine.pfnIndexOfEdict(entity), entity->serialnumber, entity == prepared);
    std::fflush(stdout);
}

static void userinfo(edict_t *entity, char *) {
    ++userinfo_calls;
    last_userinfo = entity;
    log_client("userinfo", entity);
    if (!allocation_reentry || change_requested) return;
    change_requested = true;
    if (entity->pvPrivateData || !(entity->v.flags & FL_FAKECLIENT) || map_activations != 1) {
        std::puts("FAKE_REENTRY FAIL allocation callback lacked an uninitialized fake client");
        std::fflush(stdout);
        return;
    }
    std::puts("FAKE_REENTRY EVENT userinfo during allocation; private data absent");
    std::fflush(stdout);
    engine.pfnServerCommand("changelevel rc_deathmatch\n");
    engine.pfnServerExecute();
    // The original edict can belong to the retired arena. Do not read it.
    std::puts(map_activations == 2
        ? "FAKE_REENTRY EVENT second map activated before userinfo returned"
        : "FAKE_REENTRY FAIL changelevel did not reenter during allocation");
    std::fflush(stdout);
}

static qboolean connect(edict_t *entity, const char *, const char *, char reason[128]) {
    ++connect_calls;
    log_client("connect-reject", entity);
    std::strcpy(reason, "fixture rejected setup");
    return 0;
}

static void put(edict_t *entity) {
    ++put_calls;
    log_client("put", entity);
}

static void disconnect(edict_t *entity) {
    ++disconnect_calls;
    last_disconnected = entity;
    log_client("disconnect", entity);
    if (allocation_reentry) {
        std::puts(entity->pvPrivateData
            ? "FAKE_REENTRY FAIL disconnect unexpectedly found private data"
            : "FAKE_REENTRY EVENT fixture disconnected client without private data");
        std::fflush(stdout);
    }
    if (prepared == entity) prepared = nullptr;
}

static void keyvalue(edict_t *entity, KeyValueData *data) {
    if (std::strcmp(data->szKeyName, "classname") == 0) {
        entity->v.classname = engine.pfnAllocString(data->szValue);
    }
    data->fHandled = 1;
}

static void activate(edict_t *, int, int) { active = true; ++map_activations; }
static void deactivate() { active = false; }

static void message_frame() {
    const int outer = engine.pfnRegUserMsg("SDKOuter", 1);
    const int nested = engine.pfnRegUserMsg("SDKNested", 1);
    if (outer <= 0 || nested <= 0) {
        std::puts("MESSAGE_PROBE FAIL registration");
        std::fflush(stdout);
        return;
    }
    std::puts("MESSAGE_PROBE outer begin");
    std::fflush(stdout);
    engine.pfnMessageBegin(MSG_ALL, outer, nullptr, nullptr);
    engine.pfnWriteByte(17);
    engine.pfnMessageEnd();
    std::puts("MESSAGE_PROBE outer completed");
    std::fflush(stdout);
}

static void start_frame() {
    if (allocation_reentry || attempted || !active) return;
    attempted = true;
    if (message_probe) return message_frame();
    const int capacity = globals->maxClients;
    if (capacity < 2 || capacity > 8) {
        std::puts("FAKE_FIXTURE FAIL capacity");
        return;
    }
    edict_t *first = nullptr;
    bool userinfo_order = true;
    for (int i = 0; i < capacity; ++i) {
        char name[32];
        std::snprintf(name, sizeof name, "SDKFixture%d", i);
        const int before = userinfo_calls;
        edict_t *entity = engine.pfnCreateFakeClient(name);
        if (!entity) {
            std::puts("FAKE_FIXTURE FAIL early-null");
            return;
        }
        userinfo_order &= userinfo_calls == before + 1 && last_userinfo == entity;
        log_client("created", entity);
        if (i == 0) first = entity;
    }
    const bool full = engine.pfnCreateFakeClient("SDKFixtureOverflow") == nullptr;
    const bool allocation_only = connect_calls == 0 && put_calls == 0;
    char reason[128]{};
    // Deliberately exercise the fixture's failed setup path. No game-private
    // player object was created and disconnect must tolerate that state.
    const bool refused = !connect(first, "SDKFixture0", "loopback", reason);
    const bool still_full = engine.pfnCreateFakeClient("SDKFixtureAfterReject") == nullptr;
    const bool no_extra_callbacks = userinfo_calls == capacity && connect_calls == 1 && put_calls == 0;
    int original_slot = engine.pfnIndexOfEdict(first);
    const int original_serial = first->serialnumber;
    char command[48];
    std::snprintf(command, sizeof command, "kick #%d\n", engine.pfnGetPlayerUserId(first));
    engine.pfnServerCommand(command);
    engine.pfnServerExecute();
    const bool dropped = disconnect_calls == 1 && last_disconnected == first;
    const int before = userinfo_calls;
    edict_t *replacement = engine.pfnCreateFakeClient("SDKFixtureReplacement");
    const bool reused = replacement && engine.pfnIndexOfEdict(replacement) == original_slot;
    if (replacement) {
        log_client("replacement", replacement);
        prepared = replacement;
        put(replacement);
    }
    userinfo_order &= replacement && userinfo_calls == before + 1 && last_userinfo == replacement;
    const bool same_edict = first == replacement;
    const bool same_serial = replacement && replacement->serialnumber == original_serial;
    const bool pass = full && allocation_only && refused && still_full &&
                      no_extra_callbacks && dropped && reused && userinfo_order;
    std::printf("FAKE_FIXTURE %s capacity=%d full=%d allocation_only=%d refused=%d still_full=%d "
                "disconnect=%d reused=%d userinfo_order=%d same_edict=%d same_serial=%d\n",
                pass ? "PASS" : "FAIL", capacity, full, allocation_only, refused,
                still_full, dropped, reused, userinfo_order, same_edict, same_serial);
    std::fflush(stdout);
}

extern "C" void GiveFnptrsToDll(enginefuncs_t *functions, globalvars_t *vars) {
    engine = *functions;
    globals = vars;
    message_probe = std::getenv("METAMOD_MESSAGE_PROBE") != nullptr;
    allocation_reentry = std::getenv("METAMOD_FAKE_ALLOCATION_REENTRY") != nullptr;
}

// An empty world is sufficient. Unknown map entities are discarded by the host.
extern "C" void worldspawn(entvars_t *) {}

extern "C" int GetEntityAPI2(DLL_FUNCTIONS *out, int *version) {
    if (*version != INTERFACE_VERSION) {
        *version = INTERFACE_VERSION;
        return 0;
    }
    *out = {};
#define DEFAULT(slot) out->slot = Stub<decltype(out->slot)>::call
    DEFAULT(pfnGameInit);
    DEFAULT(pfnSpawn);
    DEFAULT(pfnThink);
    DEFAULT(pfnTouch);
    DEFAULT(pfnBlocked);
    out->pfnKeyValue = keyvalue;
    DEFAULT(pfnSetAbsBox);
    DEFAULT(pfnPlayerPreThink);
    DEFAULT(pfnPlayerPostThink);
    DEFAULT(pfnPM_Init);
    DEFAULT(pfnPM_Move);
    DEFAULT(pfnSetupVisibility);
    DEFAULT(pfnAddToFullPack);
    DEFAULT(pfnCreateBaseline);
    DEFAULT(pfnCreateInstancedBaselines);
    DEFAULT(pfnRegisterEncoders);
    DEFAULT(pfnUpdateClientData);
    DEFAULT(pfnGetWeaponData);
    DEFAULT(pfnCmdStart);
    DEFAULT(pfnCmdEnd);
    DEFAULT(pfnGetHullBounds);
    DEFAULT(pfnAllowLagCompensation);
#undef DEFAULT
    out->pfnGetGameDescription = []() -> const char * { return "SDK fake-client fixture"; };
    out->pfnServerActivate = activate;
    out->pfnServerDeactivate = deactivate;
    out->pfnStartFrame = start_frame;
    out->pfnClientConnect = connect;
    out->pfnClientPutInServer = put;
    out->pfnClientDisconnect = disconnect;
    out->pfnClientUserInfoChanged = userinfo;
    return 1;
}

extern "C" int GetEntityAPI(DLL_FUNCTIONS *out, int version) {
    return GetEntityAPI2(out, &version);
}
