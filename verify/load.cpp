#include <cassert>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <limits>
#include <string>
#include <thread>
#include <extdll.h>
#include <meta_api.h>
#include <h_export.h>

#if defined(__linux__)
#include <dlfcn.h>
#endif

#ifdef METAMOD_LOADER_LSAN
#include <sanitizer/lsan_interface.h>
#endif

static_assert(sizeof(void *) == 4, "build the loader for x86");

static plugin_info_t *info;
static meta_globals_t globals{};
static DLL_FUNCTIONS hooks{};
static DLL_FUNCTIONS post_hooks{};
static NEW_DLL_FUNCTIONS new_hooks{};
static edict_t player{}, world{}, vacant{};
static bool discovery_transition;
static bool retire_discovery_edict;
static bool expect_retired_edict;
static int retired_edict_reports;
static int logged;
static int keyvalue_logs;
static int admission_deferrals;
static int admission_rejections;
static bool engine_demo;
static bool runtime_audit;
static bool audit_reentry;
static int audit_logs;
static void (*audit_status)();
static void (*audit_run)();
static void (*audit_report)();
static std::string audit_output;

static cvar_t *registered_cvar;
static const char *retained_cvar_name;
static char *retained_cvar_default;
static std::string retained_cvar_name_copy;
static std::string retained_cvar_default_copy;
static char cvar_value[1024];
static bool invalidate_cvar_string;
static bool expect_invalidated_cvar;
static int invalidated_cvar_reports;
static void (*command)();
static void (*trace_command)();
static void (*entities_command)();
static void (*relationships_command)();
static int relationship_reports;
static const char *expected_relationships;
static bool trace_transition;
static bool expect_inactive_trace;
static bool shutdown_from_trace;
static int trace_index_lookups;
static const char *args[] = {"RUST_HELLO", nullptr, nullptr};
static bool reenter_argv;
static bool expire_player;
static bool reuse_player_during_lookup;
static bool expect_reused_player_fallback;
static int userid_reads;
enum class JoinIndexChange { none, reuse, free, disconnect, map, out_of_range };
static JoinIndexChange join_index_change;

static cvar_t *find_cvar(const char *name) {
    return registered_cvar && std::strcmp(name, registered_cvar->name) == 0 ? registered_cvar : nullptr;
}

#if defined(__linux__)
static void assert_plugin_address(const void *address) {
    Dl_info plugin{}, owner{};
    assert(dladdr(info, &plugin) != 0);
    assert(dladdr(address, &owner) != 0);
    assert(owner.dli_fbase == plugin.dli_fbase);
}
#endif

static void register_cvar(cvar_t *cvar) {
    assert(!registered_cvar && std::strcmp(cvar->name, runtime_audit ? "mm_audit_value" : "rust_hello_message") == 0);
    assert(cvar->flags == 0 && cvar->next == nullptr);
#if defined(__linux__)
    assert_plugin_address(cvar);
#endif
#ifdef METAMOD_LOADER_LSAN
    // The SDK retains these allocations across shutdown. Mark the initial
    // string before the engine replaces its pointer in the record.
    __lsan_ignore_object(cvar->name);
    __lsan_ignore_object(cvar->string);
#endif
    retained_cvar_name = cvar->name;
    // The SDK registration contract supplies writable retained storage even
    // though the upstream ABI exposes string as const char*.
    retained_cvar_default = const_cast<char *>(cvar->string);
    retained_cvar_name_copy = cvar->name;
    retained_cvar_default_copy = cvar->string;
    registered_cvar = cvar;
    std::strcpy(cvar_value, cvar->string);
    cvar->string = cvar_value;
}

static const char *get_cvar(const char *name) {
    assert(find_cvar(name));
    if (invalidate_cvar_string) {
        invalidate_cvar_string = false;
        // Model a getter post hook expiring the selected value during SDK
        // reentry. No fixture access to the string occurs after delete[].
        char *selected = new char[32];
        std::strcpy(selected, "expired native value");
        hooks.pfnStartFrame();
        delete[] selected;
        return selected;
    }
    return registered_cvar->string;
}

static void set_cvar(const char *name, const char *value) {
    assert(find_cvar(name));
    std::strcpy(cvar_value, value);
}

static void (*state_command)();
static int state_prints;
static bool invalid_state;
static int state_unavailable;

static void register_command(const char *name, void (*callback)()) {
    assert(callback);
#if defined(__linux__)
    assert_plugin_address(reinterpret_cast<const void *>(callback));
#endif
#ifdef METAMOD_LOADER_LSAN
    __lsan_ignore_object(name);
#endif
    if (runtime_audit) {
        void (**slot)() = nullptr;
        if (std::strcmp(name, "mm_audit_status") == 0) slot = &audit_status;
        if (std::strcmp(name, "mm_audit_run") == 0) slot = &audit_run;
        if (std::strcmp(name, "mm_audit_report") == 0) slot = &audit_report;
        assert(slot && !*slot);
        *slot = callback;
    } else if (std::strcmp(name, "rust_state") == 0) {
        assert(!state_command);
        state_command = callback;
    } else if (std::strcmp(name, "rust_trace") == 0) {
        assert(!trace_command);
        trace_command = callback;
    } else if (std::strcmp(name, "rust_relationships") == 0) {
        assert(!relationships_command);
        relationships_command = callback;
    } else if (std::strcmp(name, "rust_entities") == 0) {
        assert(!entities_command);
        entities_command = callback;
    } else {
        assert(!command && std::strcmp(name, "rust_hello") == 0);
        command = callback;
    }
}

static int command_argc() {
    return args[2] ? 3 : args[1] ? 2 : 1;
}

static const char *command_argv(int index) {
    assert(index >= 0 && index < command_argc());
    if (reenter_argv) {
        reenter_argv = false;
        command();
    }
    return args[index];
}

static edict_t *entity_of_index(int index) {
    assert(index >= 0 && index < 8);
    if (index == 3 && retire_discovery_edict) {
        retire_discovery_edict = false;
        edict_t *retired = new edict_t{};
        // Notify retirement before freeing storage, then return the address
        // selected by the interrupted lookup. The SDK must not read it.
        hooks.pfnServerDeactivate();
        delete retired;
        return retired;
    }
    if (index == 3 && discovery_transition) {
        hooks.pfnServerDeactivate();
    }
    if (index == 0) return &world;
    if (index == 5) return &vacant;
    if (index == 7 && reuse_player_during_lookup) {
        reuse_player_during_lookup = false;
        ++player.serialnumber;
    }
    return index == 7 ? &player : nullptr;
}

static bool string_transition;

static const char *string_from_index(int index) {
    static char value[32];
    assert(index == 1 || index == 2 || index == -3 || index == 10 || index == 11);
    if (string_transition) {
        hooks.pfnServerDeactivate();
    }
    if (index == 10) return "fixture_map";
    if (index == 11) return "fixture_start";
    std::strcpy(value, index == 1 ? "worldspawn" : index == 2 ? "player" : "demo_target");
    return value;
}

static int engine_user_id = 42;

static int player_userid(edict_t *edict) {
    assert(edict == &player);
    ++userid_reads;
    return engine_user_id;
}

static const char *player_authid(edict_t *edict) {
    assert(edict == &player);
    if (expire_player) {
        expire_player = false;
        hooks.pfnClientDisconnect(edict);
    }
    return "STEAM_ID_TEST";
}

static void trace_line(const float *start, const float *end, int mode, edict_t *skip, TraceResult *out) {
    assert(!expect_inactive_trace);
    assert(start[2] == 64 && end[2] == -64 && mode == dont_ignore_monsters && !skip);
    out->flFraction = 0.5f;
    out->vecEndPos.z = 0;
    out->vecPlaneNormal.z = 1;
    out->pHit = &player;
    if (shutdown_from_trace) {
        shutdown_from_trace = false;
        new_hooks.pfnGameShutdown();
        assert(globals.mres == MRES_IGNORED);
    } else if (trace_transition) {
        hooks.pfnServerDeactivate();
    }
}

static void trace_hull(const float *start, const float *end, int mode, int hull, edict_t *skip, TraceResult *out) {
    assert(start[2] == 64 && end[2] == -64 && hull == human_hull);
    assert(runtime_audit ? (mode == dont_ignore_monsters && !skip) :
        (mode == ignore_monsters && skip == &player));
    out->flFraction = 1.0f;
}

static int index_of_edict(const edict_t *edict) {
    assert(edict == &player);
    ++trace_index_lookups;
    const auto change = join_index_change;
    join_index_change = JoinIndexChange::none;
    switch (change) {
    case JoinIndexChange::reuse: ++player.serialnumber; break;
    case JoinIndexChange::free: player.free = 1; break;
    case JoinIndexChange::disconnect: hooks.pfnClientDisconnect(&player); break;
    case JoinIndexChange::map:
        hooks.pfnServerDeactivate();
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        break;
    case JoinIndexChange::out_of_range: return 8;
    case JoinIndexChange::none: break;
    }
    return 7;
}

static void server_print(const char *message) {
    if (expected_relationships) {
        assert(std::strcmp(message, expected_relationships) == 0);
        expected_relationships = nullptr;
        ++relationship_reports;
        return;
    }
    if (runtime_audit) {
        assert(std::strncmp(message, "MM_AUDIT ", 9) == 0);
        ++audit_logs;
        audit_output += message;
        std::fputs(message, stdout);
        if (audit_reentry && std::strstr(message, "\"kind\":\"ready\"")) {
            audit_reentry = false;
            const auto before = audit_logs;
            post_hooks.pfnClientPutInServer(&player);
            assert(audit_logs == before);
        }
        return;
    }
    if (std::strncmp(message, "Server state: ", 14) == 0) {
        ++state_prints;
        if (invalid_state) {
            for (const char *field : {"{ time: None,", "frame_time: None,", "max_clients: None,", "max_entities: None,"})
                assert(std::strstr(message, field));
            return;
        }
        assert(std::strstr(message, "{ time: Some(12.5),"));
        assert(std::strstr(message, "frame_time: Some(0.02)"));
        assert(std::strstr(message, "max_clients: Some(7)"));
        assert(std::strstr(message, "max_entities: Some(8)"));
        assert(std::strstr(message, "map_name: Some(\"fixture_map\")"));
        assert(std::strstr(message, "start_spot: Some(\"fixture_start\")"));
        assert(std::strstr(message, "deathmatch: Some(2.5)"));
        assert(std::strstr(message, "coop: Some(-1.0)"));
        assert(std::strstr(message, "teamplay: Some(3.0)"));
        assert(std::strstr(message, "server_flags: Some(16.0)"));
        return;
    }
    if (std::strcmp(message, "Server state unavailable\n") == 0) {
        ++state_unavailable;
        return;
    }
    if (expect_retired_edict) {
        assert(!retire_discovery_edict);
        assert(std::strcmp(message, "Entity discovery failed: the map changed during entity discovery\n") == 0);
        expect_retired_edict = false;
        ++retired_edict_reports;
        return;
    }
    if (expect_invalidated_cvar) {
        assert(!invalidate_cvar_string);
        assert(std::strcmp(message, "Could not read rust_hello_message: engine operation unavailable\n") == 0);
        expect_invalidated_cvar = false;
        ++invalidated_cvar_reports;
        return;
    }
    if (expect_inactive_trace) {
        assert(std::strcmp(message, "Line trace failed: tracing requires an active map and engine callbacks\n") == 0);
        expect_inactive_trace = false;
        return;
    }
    if (expect_reused_player_fallback) {
        assert(!reuse_player_during_lookup);
        assert(std::strcmp(message, "Hello, player in slot 7!\n") == 0);
        expect_reused_player_fallback = false;
        return;
    }
    if (std::strcmp(message, "Rust admission demo: deferred\n") == 0) {
        assert(engine_demo && ++admission_deferrals == 1);
        return;
    }
    if (std::strcmp(message, "Rust admission demo: rejected\n") == 0) {
        assert(engine_demo && ++admission_rejections == 1);
        char nested[128];
        std::memset(nested, '!', sizeof nested);
        globals.mres = MRES_UNSET;
        assert(hooks.pfnClientConnect(&player, "RustAdmissionReject", "loopback", nested) == 1);
        assert(globals.mres == MRES_IGNORED);
        for (char byte : nested) assert(byte == '!');
        return;
    }
    if (std::strncmp(message, "Map classname: ", 15) == 0) {
        assert(engine_demo && keyvalue_logs < 2);
        assert(logged == (keyvalue_logs == 0 ? 1 : 4));
        assert(std::strcmp(message, keyvalue_logs == 0
            ? "Map classname: worldspawn\n" : "Map classname: deferred\n") == 0);
        ++keyvalue_logs;
        std::fputs(message, stdout);
        return;
    }
    static bool hello_newline = false;
    if (!engine_demo && logged == 1 && !hello_newline) {
        assert(std::strcmp(message, "\n") == 0);
        hello_newline = true;
        std::fputs(message, stdout);
        return;
    }
    static const char *hello_output[] = {
        "Hello from Rust Metamod",
        "Rust StartFrame hook ran\n",
        "Hello, player in slot 7!\n",
    };
    static const char *engine_output[] = {
        "Rust Engine Demo loaded\n",
        "Rust Engine Demo first frame\n",
        "Hello, player in slot 7 (user ID 42)!\n",
        "Deferred command argument\377\n",
        "Hello, player in slot 7!\n",
        "Deferred command argument\377\n",
        "New value\n",
        "Visible entities: 2\n",
        "Entity 0 World class=worldspawn targetname=<unset> origin: [0.0, 0.0, 0.0]\n",
        "Entity 7 FakeClient class=player targetname=demo_target origin: [1.0, 2.0, 3.0]\n",
        "Visible entities: 0\n",
        "Entity discovery failed: the map changed during entity discovery\n",
        "Visible entities: 1\n",
        "Line fraction: 0.5, hit: Some(7)\n",
        "Hull fraction: 1\n",
        "Line trace failed: the map changed during the trace\n",
    };
    assert(logged < (engine_demo ? 16 : 3));
    assert(std::strcmp(message, engine_demo ? engine_output[logged] : hello_output[logged]) == 0);
    std::fputs(message, stdout);
    if (++logged == 2) {
        // Both notifications are queued while the frame owns plugin state.
        globals.mres = MRES_UNSET;
        post_hooks.pfnClientPutInServer(&player);
        assert(logged == 2 && globals.mres == MRES_IGNORED);
        globals.mres = MRES_SUPERCEDE;
        if (!engine_demo) {
            assert(!command && !registered_cvar);
            return;
        }
        char transient[] = "Deferred command argument\377";
        args[1] = transient;
        assert(command);
        command();
        assert(logged == 2 && globals.mres == MRES_SUPERCEDE);
        transient[0] = 'X';
        args[1] = nullptr;
        char key[] = "classname";
        char value[] = "deferred";
        KeyValueData data{"fixture", key, value, 17};
        hooks.pfnKeyValue(&player, &data);
        assert(data.fHandled == 17 && keyvalue_logs == 1);
        assert(globals.mres == MRES_IGNORED);
        key[0] = 'X';
        value[0] = 'X';
    }
}

static void metamod_console(plid_t plugin, const char *format, ...) {
    assert(engine_demo && plugin == info && std::strcmp(format, "%s") == 0);
    va_list args;
    va_start(args, format);
    const char *message = va_arg(args, const char *);
    va_end(args);
    assert(std::strcmp(message, "Rust Engine Demo loaded") == 0);
    // Metamod's console utility appends the newline.
    server_print("Rust Engine Demo loaded\n");
}

template <typename Fn>
static Fn symbol(DLHANDLE module, const char *name) {
    auto function = reinterpret_cast<Fn>(DLSYM(module, name));
    assert(function);
    return function;
}

// All arguments are fixed fixture literals.
static void native_pass(const char *test, const char *evidence) {
    std::printf("MM_AUDIT_NATIVE {\"schema_version\":1,\"test\":\"%s\",\"result\":\"PASS\",\"observed\":\"%s\"}\n",
                test, evidence);
    std::fflush(stdout);
}

static void exercise_audit() {
    assert(hooks.pfnKeyValue && hooks.pfnAllowLagCompensation && post_hooks.pfnAllowLagCompensation);
    KeyValueData data{nullptr, "classname", "worldspawn", 17};
    hooks.pfnKeyValue(&world, &data);
    assert(data.fHandled == 17);
    hooks.pfnServerActivate(nullptr, 0, 0);
    post_hooks.pfnServerActivate(nullptr, 0, 0);
    audit_reentry = true;
    hooks.pfnStartFrame();
    assert(!audit_reentry && audit_status && audit_run && audit_report);
    assert(audit_output.find("\"test\":\"player.join.v1\",\"result\":\"PASS\"") != std::string::npos);
    args[0] = "mm_audit_status";
    audit_status();
    hooks.pfnClientDisconnect(&player);
    ++player.serialnumber;
    post_hooks.pfnClientPutInServer(&player);
    hooks.pfnServerDeactivate();
    hooks.pfnServerActivate(nullptr, 0, 0);
    post_hooks.pfnServerActivate(nullptr, 0, 0);
    assert(hooks.pfnAllowLagCompensation() == 0 && globals.mres == MRES_IGNORED);
    int original = 1;
    globals.orig_ret = &original;
    assert(post_hooks.pfnAllowLagCompensation() == 0 && globals.mres == MRES_IGNORED);
    globals.orig_ret = nullptr;
    args[0] = "mm_audit_run";
    args[1] = "all";
    args[2] = "audit two words";
    audit_run();
    args[0] = "mm_audit_report";
    args[1] = nullptr;
    args[2] = nullptr;
    audit_report();
    assert(audit_output.find("\"result\":\"FAIL\"") == std::string::npos);
    for (const char *expected : {"server.state.v1", "map.stale.v1", "player.reuse.v1", "entity.snapshot.v1",
             "cvar.roundtrip.v1", "command.arguments.v1", "trace.line.v1",
             "trace.hull.v1", "resources.repeat.v1"}) {
        assert(audit_output.find(std::string("\"test\":\"") + expected + "\",\"result\":\"PASS\"") != std::string::npos);
    }
    assert(!audit_reentry && audit_output.find("\"kind\":\"report\"") != std::string::npos);
    // Shutdown records use stderr because engine access is revoked.
    // Keep the combined fixture transcript line-aligned under redirection.
    std::fflush(stdout);
    new_hooks.pfnGameShutdown();
    assert(registered_cvar == find_cvar("mm_audit_value"));
    assert(registered_cvar->string == cvar_value);
    // The module remains loaded: shutdown must preserve host-retained storage.
    // In particular, replacing string must not free the original default.
    assert(registered_cvar->name == retained_cvar_name);
    assert(retained_cvar_name_copy == retained_cvar_name);
    assert(retained_cvar_default_copy == retained_cvar_default);
    const char first = retained_cvar_default[0];
    retained_cvar_default[0] = first == 'X' ? 'Y' : 'X';
    assert(retained_cvar_default[0] != first);
    retained_cvar_default[0] = first;
    assert(retained_cvar_default_copy == retained_cvar_default);
    assert(registered_cvar->flags == 0 && registered_cvar->next == nullptr);
    native_pass("native.cvar.retained_storage.v1", "Record name and replaced default survived shutdown and the original default remained writable");
    const auto before = audit_logs;
    audit_report();
    assert(audit_logs == before);
    native_pass("native.hook.result.v1", "Ignored pre and post decisions preserved fixture hook semantics with an original return slot");
    native_pass("native.dispatch.reentrant.v1", "Nested join entered from server print and was delivered after the frame callback");
    native_pass("native.shutdown.dispatch_revoked.v1", "A command after GameShutdown did not dispatch to the audit plugin");
}

int main(int argc, char **argv) {
    assert(argc == 3);
    engine_demo = std::strcmp(argv[2], "engine-demo") == 0;
    runtime_audit = std::strcmp(argv[2], "metamod-audit") == 0;
    assert(engine_demo || runtime_audit || std::strcmp(argv[2], "hello-world") == 0);
    DLHANDLE module = DLOPEN(argv[1]);
    if (!module) {
#ifdef _WIN32
        std::fprintf(stderr, "LoadLibrary failed with error %lu\n", GetLastError());
#else
        std::fprintf(stderr, "%s\n", dlerror());
#endif
        return 1;
    }
    auto give = symbol<GIVE_ENGINE_FUNCTIONS_FN>(module, "GiveFnptrsToDll");
    auto query = symbol<META_QUERY_FN>(module, "Meta_Query");
    auto attach = symbol<META_ATTACH_FN>(module, "Meta_Attach");
    auto detach = symbol<META_DETACH_FN>(module, "Meta_Detach");
    enginefuncs_t engine{};
    engine.pfnServerPrint = server_print;
    engine.pfnTraceLine = trace_line;
    engine.pfnTraceHull = trace_hull;
    engine.pfnIndexOfEdict = index_of_edict;
    engine.pfnCVarRegister = register_cvar;
    engine.pfnCVarGetPointer = find_cvar;
    engine.pfnCVarGetString = get_cvar;
    engine.pfnCVarSetString = set_cvar;
    engine.pfnAddServerCommand = register_command;
    engine.pfnCmd_Argc = command_argc;
    engine.pfnCmd_Argv = command_argv;
    engine.pfnPEntityOfEntIndex = entity_of_index;
    engine.pfnSzFromIndex = string_from_index;
    engine.pfnGetPlayerUserId = player_userid;
    engine.pfnGetPlayerAuthId = player_authid;
    globalvars_t engine_globals{};
    engine_globals.maxEntities = 8;
    engine_globals.maxClients = 7;
    engine_globals.time = 12.5f;
    engine_globals.frametime = 0.02f;
    engine_globals.mapname = 10;
    engine_globals.startspot = 11;
    engine_globals.deathmatch = 2.5f;
    engine_globals.coop = -1.0f;
    engine_globals.teamplay = 3.0f;
    engine_globals.serverflags = 16.0f;
    player.v.origin.x = 1;
    player.v.origin.y = 2;
    player.v.origin.z = 3;
    vacant.free = 1;
    world.v.classname = 1;
    player.v.classname = 2;
    player.v.targetname = static_cast<string_t>(-3);
    player.v.flags = FL_CLIENT | FL_FAKECLIENT;
    META_FUNCTIONS functions{};
    gamedll_funcs_t game{};
    mutil_funcs_t utilities{};
    utilities.pfnLogConsole = metamod_console;
    assert(!query(META_INTERFACE_VERSION, &info, &utilities));
    give(&engine, &engine_globals);
    std::thread foreign_loader([&] {
        enginefuncs_t other_engine{};
        give(&other_engine, nullptr);
        plugin_info_t *foreign_info = nullptr;
        assert(!query(META_INTERFACE_VERSION, &foreign_info, &utilities));
        assert(!foreign_info);
        assert(!attach(PT_STARTUP, &functions, &globals, &game));
    });
    foreign_loader.join();
    assert(!attach(PT_STARTUP, &functions, &globals, &game));
    assert(!query("5:12", &info, &utilities));
    assert(!query("6:13", &info, &utilities));
    assert(!query("5:13x", &info, &utilities));
    assert(query("5:14", &info, &utilities));
    auto *saved_info = info;
    assert(query(META_INTERFACE_VERSION, &info, &utilities));
    assert(info == saved_info);
    assert(std::strcmp(info->name, runtime_audit ? "Rust Runtime Audit" : engine_demo ? "Rust Engine Demo" : "Rust Hello World") == 0);
    assert(std::strcmp(info->logtag, runtime_audit ? "RUSTAUDIT" : engine_demo ? "RUSTENGINE" : "RUSTHELLO") == 0);
    if (engine_demo) {
        assert(std::strcmp(info->version, "0.1.0") == 0);
        assert(std::strcmp(info->author, "metamod-sdk-rs contributors") == 0);
    }
    assert(info->loadable == PT_STARTUP && info->unloadable == PT_NEVER);
    assert(!attach(PT_ANYTIME, &functions, &globals, &game));
    assert(attach(PT_STARTUP, &functions, &globals, &game));
    assert(!attach(PT_STARTUP, &functions, &globals, &game));
    assert(functions.pfnGetEntityAPI2 && functions.pfnGetEntityAPI2_Post);
    int version = 0;
    assert(!functions.pfnGetEntityAPI2(&hooks, &version));
    assert(version == INTERFACE_VERSION && !hooks.pfnStartFrame);
    assert(functions.pfnGetEntityAPI2(&hooks, &version));
    version = 0;
    assert(!functions.pfnGetEntityAPI2_Post(&post_hooks, &version));
    assert(version == INTERFACE_VERSION && !post_hooks.pfnServerActivate);
    assert(functions.pfnGetEntityAPI2_Post(&post_hooks, &version));
    assert(post_hooks.pfnClientPutInServer && !post_hooks.pfnClientDisconnect);
    assert(hooks.pfnClientDisconnect && !hooks.pfnClientPutInServer);
    assert(post_hooks.pfnServerActivate && hooks.pfnServerActivate && hooks.pfnServerDeactivate);
    version = 0;
    assert(functions.pfnGetNewDLLFunctions && !functions.pfnGetNewDLLFunctions(&new_hooks, &version));
    assert(version == NEW_DLL_FUNCTIONS_VERSION && !new_hooks.pfnGameShutdown);
    assert(functions.pfnGetNewDLLFunctions(&new_hooks, &version));
    assert(new_hooks.pfnGameShutdown && !new_hooks.pfnShouldCollide);
    assert((hooks.pfnClientConnect != nullptr) == engine_demo && !post_hooks.pfnClientConnect);
    if (engine_demo) {
        char reason[129];
        std::memset(reason, '!', sizeof reason);
        globals.prev_mres = MRES_HANDLED;
        assert(hooks.pfnClientConnect(&player, "RustAdmissionAllow", "loopback", reason) == 1);
        assert(globals.mres == MRES_IGNORED && globals.prev_mres == MRES_HANDLED);
        char unterminated[4096];
        std::memset(unterminated, 'x', sizeof unterminated);
        assert(hooks.pfnClientConnect(&player, unterminated, "loopback", reason) == 1);
        assert(hooks.pfnClientConnect(&player, nullptr, "loopback", reason) == 1);
        assert(hooks.pfnClientConnect(&player, "RustAdmissionReject", nullptr, reason) == 1);
        assert(hooks.pfnClientConnect(&player, "RustAdmissionReject", "loopback", nullptr) == 1);
        assert(globals.mres == MRES_IGNORED);
        for (char byte : reason) assert(byte == '!');
        assert(hooks.pfnClientConnect(&player, "RustAdmissionReject", "loopback", reason) == 0);
        assert(globals.mres == MRES_SUPERCEDE && globals.prev_mres == MRES_HANDLED);
        assert(std::strcmp(reason, "Rust admission demo rejected this name.") == 0);
        assert(reason[128] == '!');
        assert(admission_deferrals == 1 && admission_rejections == 1);
        globals.prev_mres = MRES_UNSET;
    }
    if (runtime_audit) {
        assert(!detach(PT_ANYTIME, PNL_COMMAND));
        exercise_audit();
        return 0;
    }
    assert((hooks.pfnKeyValue != nullptr) == engine_demo && !post_hooks.pfnKeyValue);
    if (engine_demo) {
        KeyValueData data{nullptr, "classname", "worldspawn", 17};
        hooks.pfnKeyValue(&world, &data);
        assert(keyvalue_logs == 1 && data.fHandled == 17 && globals.mres == MRES_IGNORED);
        char oversized[4097];
        std::memset(oversized, 'x', sizeof(oversized));
        oversized[sizeof(oversized) - 1] = '\0';
        data.szValue = oversized;
        hooks.pfnKeyValue(&world, &data);
        data.szValue = nullptr;
        hooks.pfnKeyValue(&world, &data);
        hooks.pfnKeyValue(&world, nullptr);
        assert(keyvalue_logs == 1 && data.fHandled == 17 && globals.mres == MRES_IGNORED);
    }
    hooks.pfnServerActivate(nullptr, 0, 0);
    globals.mres = MRES_UNSET;
    post_hooks.pfnServerActivate(nullptr, 0, 0);
    assert(globals.mres == MRES_IGNORED);
    assert(hooks.pfnStartFrame && !hooks.pfnThink);
    assert((hooks.pfnAllowLagCompensation != nullptr) == engine_demo);
    assert((post_hooks.pfnAllowLagCompensation != nullptr) == engine_demo);
    if (engine_demo) {
        assert(hooks.pfnAllowLagCompensation() == 0 && globals.mres == MRES_HANDLED);
        int original = 1;
        globals.orig_ret = &original;
        globals.status = MRES_HANDLED;
        assert(post_hooks.pfnAllowLagCompensation() == 0 && globals.mres == MRES_HANDLED);
        globals.status = MRES_SUPERCEDE;
        assert(post_hooks.pfnAllowLagCompensation() == 0 && globals.mres == MRES_IGNORED);
        globals.orig_ret = nullptr;
        globals.status = MRES_UNSET;
    }
    globals.mres = MRES_SUPERCEDE;
    std::thread foreign_hook([] { hooks.pfnStartFrame(); });
    foreign_hook.join();
    assert(globals.mres == MRES_SUPERCEDE && logged == 1);
    for (int frame = 0; frame < 2; ++frame) {
        globals.mres = MRES_UNSET;
        hooks.pfnStartFrame();
        assert(globals.mres == MRES_IGNORED);
    }
    if (engine_demo) {
        assert(state_command);
        args[0] = "rust_state";
        args[1] = nullptr;
        state_command();
        assert(state_prints == 1);
        invalid_state = true;
        engine_globals.time = std::numeric_limits<float>::quiet_NaN();
        engine_globals.frametime = -1.0f;
        engine_globals.maxClients = -1;
        engine_globals.maxEntities = 0;
        state_command();
        assert(state_prints == 2);
        invalid_state = false;
        engine_globals.time = 12.5f;
        engine_globals.frametime = 0.02f;
        engine_globals.maxClients = 7;
        engine_globals.maxEntities = 8;
        string_transition = true;
        state_command();
        assert(state_unavailable == 1);
        string_transition = false;
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        args[0] = "rust_hello";
        assert(logged == 4);
        globals.mres = MRES_UNSET;
        post_hooks.pfnClientPutInServer(nullptr);
        assert(logged == 4 && globals.mres == MRES_IGNORED);
        // Keep the fixture allocation live, including during map transitions.
        const JoinIndexChange rejected_joins[] = {
            JoinIndexChange::reuse, JoinIndexChange::free,
            JoinIndexChange::disconnect, JoinIndexChange::map, JoinIndexChange::out_of_range,
        };
        for (auto change : rejected_joins) {
            const int reads_before = userid_reads;
            join_index_change = change;
            globals.mres = MRES_UNSET;
            post_hooks.pfnClientPutInServer(&player);
            assert(join_index_change == JoinIndexChange::none);
            assert(logged == 4 && userid_reads == reads_before);
            assert(globals.mres == MRES_IGNORED);
            player.free = 0;
        }
        const int reads_before_reuse = userid_reads;
        reuse_player_during_lookup = true;
        expect_reused_player_fallback = true;
        post_hooks.pfnClientPutInServer(&player);
        assert(!expect_reused_player_fallback && userid_reads == reads_before_reuse);
        globals.mres = MRES_UNSET;
        engine_user_id = -1;
        post_hooks.pfnClientPutInServer(&player);
        assert(logged == 5 && globals.mres == MRES_IGNORED);
        expire_player = true;
        post_hooks.pfnClientPutInServer(&player);
        assert(!expire_player && logged == 5);
        reenter_argv = true;
        command();
        assert(!reenter_argv && logged == 5);
        invalidate_cvar_string = true;
        expect_invalidated_cvar = true;
        command();
        assert(!invalidate_cvar_string && !expect_invalidated_cvar);
        assert(invalidated_cvar_reports == 1 && logged == 5);
        globals.mres = MRES_SUPERCEDE;
        command();
        assert(logged == 6 && globals.mres == MRES_SUPERCEDE);
        native_pass("native.string.reentrant_invalidation.v1", "Getter returned freed storage after SDK reentry; read reported unavailable and a later ordinary read succeeded");
        args[1] = "New value";
        command();
        args[1] = nullptr;
        assert(logged == 7 && std::strcmp(cvar_value, "New value") == 0);
        assert(entities_command);
        args[0] = "rust_entities";
        entities_command();
        args[1] = "8";
        entities_command();
        args[1] = nullptr;
        discovery_transition = true;
        entities_command();
        discovery_transition = false;
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        assert(logged == 12);
        retire_discovery_edict = true;
        expect_retired_edict = true;
        entities_command();
        assert(!retire_discovery_edict && !expect_retired_edict);
        assert(retired_edict_reports == 1 && logged == 12);
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        native_pass("native.entity.retired_arena.v1", "Discovery discarded a freed edict returned after map retirement without publishing a stale snapshot");
        args[1] = "7";
        string_transition = true;
        entities_command();
        string_transition = false;
        args[1] = nullptr;
        assert(logged == 13);
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        assert(trace_command);
        assert(relationships_command);
        args[0] = "rust_relationships";
        args[1] = "0";
        world.v.owner = &player;
        // An unrelated address must only be compared, never dereferenced or
        // sent to the pointer-to-index callback.
        world.v.enemy = reinterpret_cast<edict_t *>(&logged);
        world.v.aiment = &world;
        world.v.groundentity = &vacant;
        expected_relationships = "Relationships 0 owner=Some(7) enemy=None aiment=Some(0) ground=None\n";
        relationships_command();
        assert(!expected_relationships && relationship_reports == 1);
        player.free = 1;
        expected_relationships = "Relationships 0 owner=None enemy=None aiment=Some(0) ground=None\n";
        relationships_command();
        assert(!expected_relationships && relationship_reports == 2);
        player.free = 0;
        discovery_transition = true;
        expected_relationships = "Entity relationships unavailable\n";
        relationships_command();
        assert(!expected_relationships && relationship_reports == 3);
        discovery_transition = false;
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        world.v.owner = world.v.enemy = world.v.aiment = world.v.groundentity = nullptr;
        args[1] = nullptr;
        args[0] = "rust_trace";
        // An inner activation cannot publish an arena while the outer
        // GameDLL activation can still replace it.
        hooks.pfnServerActivate(nullptr, 0, 0);
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        args[0] = "rust_state";
        state_command();
        assert(state_unavailable == 2);
        args[0] = "rust_trace";
        expect_inactive_trace = true;
        trace_command();
        assert(!expect_inactive_trace && logged == 13);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        trace_command();
        assert(logged == 15);
        trace_transition = true;
        trace_command();
        assert(logged == 16);
        // Model a deactivation reentered between the pre and post phases.
        hooks.pfnServerActivate(nullptr, 0, 0);
        hooks.pfnServerDeactivate();
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        expect_inactive_trace = true;
        trace_command();
        assert(!expect_inactive_trace && logged == 16);
    } else {
        assert(logged == 3 && !command && !registered_cvar);
    }
    if (engine_demo) {
        // The command budget is 4,096 bytes. Supply an initialized
        // allocation covering the full budget with no terminator.
        char unterminated[4096];
        std::memset(unterminated, 'x', sizeof(unterminated));
        const char *previous_arg = args[0];
        args[0] = unterminated;
        const int previous_logs = logged;
        command();
        assert(logged == previous_logs);
        args[0] = previous_arg;
    }
    assert(keyvalue_logs == (engine_demo ? 2 : 0));
    const int expected_logs = engine_demo ? 16 : 3;
    globals.mres = MRES_UNSET;
    hooks.pfnClientDisconnect(&player);
    assert(globals.mres == MRES_IGNORED);
    globals.mres = MRES_UNSET;
    hooks.pfnServerDeactivate();
    assert(globals.mres == MRES_IGNORED);
    assert(!detach(PT_ANYTIME, PNL_COMMAND));
    assert(!detach(PT_ANYTIME, PNL_CMD_FORCED));
    if (engine_demo) {
        // Shut down while a Rust command owns the plugin and is awaiting a
        // trace result. The hit storage stays valid in this fixture.
        hooks.pfnServerActivate(nullptr, 0, 0);
        post_hooks.pfnServerActivate(nullptr, 0, 0);
        const int previous_lookups = trace_index_lookups;
        shutdown_from_trace = true;
        trace_command();
        assert(!shutdown_from_trace && trace_index_lookups == previous_lookups);
        assert(logged == expected_logs);
    }
    globals.mres = MRES_UNSET;
    new_hooks.pfnGameShutdown();
    assert(globals.mres == MRES_IGNORED);
    globals.mres = MRES_UNSET;
    post_hooks.pfnClientPutInServer(&player);
    assert(logged == expected_logs && globals.mres == MRES_IGNORED);
    if (engine_demo) {
        char reason[128] = "unchanged";
        assert(hooks.pfnClientConnect(&player, "RustAdmissionReject", "loopback", reason) == 1);
        assert(globals.mres == MRES_IGNORED && std::strcmp(reason, "unchanged") == 0);
        command();
        assert(logged == expected_logs);
        native_pass("native.command.snapshot_limits.v1", "Nested argv read and unterminated bounded argument were discarded");
        native_pass("native.entity.reentrant_map.v1", "Nested transitions invalidated discovery string lookup and trace results");
        native_pass("native.shutdown.reentrant_trace.v1", "Nested shutdown suppressed hit lookup and further command dispatch");
    }
}
