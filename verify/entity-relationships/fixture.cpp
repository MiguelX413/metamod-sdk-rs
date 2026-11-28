// Test-only native edict control for GameDLLs exporting info_target (stock
// Ricochet in the recorded runs). Use only in isolated servers without clients.
#include <cstdio>
#include <extdll.h>
#include <meta_api.h>

static enginefuncs_t engine;
static globalvars_t *globals;
static meta_globals_t *meta;
static edict_t *source, *target;
static int target_index, target_serial;
static plugin_info_t info = {
    META_INTERFACE_VERSION, "SDK relationship fixture", "1", "",
    "metamod-sdk-rs", "", "RELNATIVE", PT_STARTUP, PT_NEVER
};

static void check(const char *phase) {
    char command[128];
    std::snprintf(command, sizeof(command), "sdk_relationship_check %d %d %s\n",
                  engine.pfnIndexOfEdict(source), target_index, phase);
    engine.pfnServerCommand(command);
}

static void prepare() {
    if (source || target) { engine.pfnServerPrint("RELATIONSHIP FAIL duplicate prepare\n"); return; }
    // Leave ample room: engine creation is fatal rather than fallible at capacity.
    if (globals->maxEntities - engine.pfnNumberOfEntities() < 128) {
        engine.pfnServerPrint("RELATIONSHIP FAIL insufficient edict capacity\n"); return;
    }
    const int classname = engine.pfnAllocString("info_target");
    source = engine.pfnCreateNamedEntity(classname);
    target = engine.pfnCreateNamedEntity(classname);
    if (!source || !target) { engine.pfnServerPrint("RELATIONSHIP FAIL allocation\n"); return; }
    source->v.nextthink = target->v.nextthink = -1;
    source->v.targetname = engine.pfnAllocString("sdk_relationship_source");
    target->v.targetname = engine.pfnAllocString("sdk_relationship_target");
    source->v.owner = source->v.enemy = target;
    source->v.aiment = source;
    source->v.groundentity = engine.pfnPEntityOfEntIndex(0);
    target_index = engine.pfnIndexOfEdict(target);
    target_serial = target->serialnumber;
    char message[192];
    std::snprintf(message, sizeof(message),
                  "RELATIONSHIP NATIVE source=%d target=%d source_visible=%d target_visible=%d source_private=%d target_private=%d\n",
                  engine.pfnIndexOfEdict(source), target_index,
                  engine.pfnPEntityOfEntIndex(engine.pfnIndexOfEdict(source)) == source,
                  engine.pfnPEntityOfEntIndex(target_index) == target,
                  source->pvPrivateData != nullptr, target->pvPrivateData != nullptr);
    engine.pfnServerPrint(message);
    check("live");
}

static void retire() {
    if (!source || !target) { engine.pfnServerPrint("RELATIONSHIP FAIL missing target\n"); return; }
    engine.pfnRemoveEntity(target);
    target = nullptr;
    // Keep incoming pointers unchanged to exercise the SDK's vacant-slot path.
    check("retired");
}

static void reuse() {
    if (!source || target || globals->maxEntities - engine.pfnNumberOfEntities() < 128) {
        engine.pfnServerPrint("RELATIONSHIP FAIL reuse preconditions\n"); return;
    }
    // The runner waits past the engine's reuse delay. Hold lower free slots
    // temporarily so repeated allocations can reach the original target slot.
    edict_t *held[64]{};
    int count = 0;
    for (; count < 64; ++count) {
        edict_t *candidate = engine.pfnCreateNamedEntity(engine.pfnAllocString("info_target"));
        if (!candidate) break;
        candidate->v.nextthink = -1;
        if (engine.pfnIndexOfEdict(candidate) == target_index) { target = candidate; break; }
        held[count] = candidate;
    }
    for (int i = 0; i < count; ++i) engine.pfnRemoveEntity(held[i]);
    if (!target || target->serialnumber == target_serial || source->v.owner != target) {
        engine.pfnServerPrint("RELATIONSHIP FAIL slot was not reused with a new serial\n"); return;
    }
    char message[160];
    std::snprintf(message, sizeof(message),
                  "RELATIONSHIP NATIVE reuse slot=%d serial_before=%d serial_after=%d\n",
                  target_index, target_serial, target->serialnumber);
    engine.pfnServerPrint(message);
    check("reused");
}

static void deactivate() {
    // Map teardown owns the allocations. Never touch saved pointers afterwards.
    source = target = nullptr;
    meta->mres = MRES_IGNORED;
}
static int hooks(DLL_FUNCTIONS *out, int *version) {
    if (*version != INTERFACE_VERSION) { *version = INTERFACE_VERSION; return 0; }
    *out = {};
    out->pfnServerDeactivate = deactivate;
    return 1;
}
extern "C" void GiveFnptrsToDll(enginefuncs_t *functions, globalvars_t *vars) {
    engine = *functions; globals = vars;
}
C_DLLEXPORT int Meta_Query(const char *, plugin_info_t **out, mutil_funcs_t *) { *out = &info; return 1; }
C_DLLEXPORT int Meta_Attach(PLUG_LOADTIME now, META_FUNCTIONS *out, meta_globals_t *vars, gamedll_funcs_t *) {
    if (now != PT_STARTUP) return 0;
    meta = vars;
    *out = {};
    out->pfnGetEntityAPI2 = hooks;
    engine.pfnAddServerCommand("sdk_relationship_prepare", prepare);
    engine.pfnAddServerCommand("sdk_relationship_retire", retire);
    engine.pfnAddServerCommand("sdk_relationship_reuse", reuse);
    return 1;
}
C_DLLEXPORT int Meta_Detach(PLUG_LOADTIME, PL_UNLOAD_REASON) { return 0; }
