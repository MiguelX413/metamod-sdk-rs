// Test-only client demo control for isolated, consenting Ricochet clients.
// No messaging hooks are installed. Commands execute from the server console.
#include <cstdio>
#include <extdll.h>
#include <meta_api.h>
static enginefuncs_t engine;
static plugin_info_t info = {
    META_INTERFACE_VERSION, "SDK delivery recording fixture", "1", "",
    "metamod-sdk-rs", "", "MSGRECORD", PT_STARTUP, PT_NEVER
};
static void command(const char *text) {
    for (int i = 1; i <= 2; ++i) {
        edict_t *entity = engine.pfnPEntityOfEntIndex(i);
        if (!entity || entity->free || !(entity->v.flags & FL_CLIENT)) continue;
        // Fixed commands only. The isolated host keeps this live edict valid
        // through synchronous ClientCommand. The native server queues stufftext.
        engine.pfnClientCommand(entity, "%s", text);
    }
}
static void record() { command("record sdk_message_delivery_20261124\n"); }
static void stop() { command("stop\n"); }
extern "C" void GiveFnptrsToDll(enginefuncs_t *functions, globalvars_t *) { engine = *functions; }
C_DLLEXPORT int Meta_Query(const char *, plugin_info_t **out, mutil_funcs_t *) {
    *out = &info;
    return 1;
}
C_DLLEXPORT int Meta_Attach(PLUG_LOADTIME now, META_FUNCTIONS *out, meta_globals_t *, gamedll_funcs_t *) {
    if (now != PT_STARTUP) return 0;
    *out = {};
    engine.pfnAddServerCommand("sdk_record_client", record);
    engine.pfnAddServerCommand("sdk_stop_recording", stop);
    return 1;
}
C_DLLEXPORT int Meta_Detach(PLUG_LOADTIME, PL_UNLOAD_REASON) { return 0; }
