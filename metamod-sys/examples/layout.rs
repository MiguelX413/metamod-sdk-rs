use metamod_sys::*;
use std::mem::{align_of, offset_of, size_of};

fn main() {
    println!(
        "PluginInfo {} {}",
        size_of::<PluginInfo>(),
        align_of::<PluginInfo>()
    );
    println!("interface {}", offset_of!(PluginInfo, ifvers));
    println!("name {}", offset_of!(PluginInfo, name));
    println!("version {}", offset_of!(PluginInfo, version));
    println!("date {}", offset_of!(PluginInfo, date));
    println!("author {}", offset_of!(PluginInfo, author));
    println!("url {}", offset_of!(PluginInfo, url));
    println!("log_tag {}", offset_of!(PluginInfo, logtag));
    println!("loadable {}", offset_of!(PluginInfo, loadable));
    println!("unloadable {}", offset_of!(PluginInfo, unloadable));
    println!(
        "MetaGlobals {} {}",
        size_of::<MetaGlobals>(),
        align_of::<MetaGlobals>()
    );
    println!("result {}", offset_of!(MetaGlobals, mres));
    println!("previous {}", offset_of!(MetaGlobals, prev_mres));
    println!("status {}", offset_of!(MetaGlobals, status));
    println!("original_return {}", offset_of!(MetaGlobals, orig_ret));
    println!("override_return {}", offset_of!(MetaGlobals, override_ret));
    println!(
        "MetaFunctions {} {}",
        size_of::<MetaFunctions>(),
        align_of::<MetaFunctions>()
    );
    println!("entity_api {}", offset_of!(MetaFunctions, pfnGetEntityAPI));
    println!(
        "entity_api_post {}",
        offset_of!(MetaFunctions, pfnGetEntityAPI_Post)
    );
    println!(
        "entity_api2 {}",
        offset_of!(MetaFunctions, pfnGetEntityAPI2)
    );
    println!(
        "entity_api2_post {}",
        offset_of!(MetaFunctions, pfnGetEntityAPI2_Post)
    );
    println!(
        "new_dll {}",
        offset_of!(MetaFunctions, pfnGetNewDLLFunctions)
    );
    println!(
        "new_dll_post {}",
        offset_of!(MetaFunctions, pfnGetNewDLLFunctions_Post)
    );
    println!(
        "engine {}",
        offset_of!(MetaFunctions, pfnGetEngineFunctions)
    );
    println!(
        "engine_post {}",
        offset_of!(MetaFunctions, pfnGetEngineFunctions_Post)
    );
    println!(
        "GameDllFunctions {} {}",
        size_of::<GameDllFunctions>(),
        align_of::<GameDllFunctions>()
    );
    println!("dll {}", offset_of!(GameDllFunctions, dllapi_table));
    println!("new_dll {}", offset_of!(GameDllFunctions, newapi_table));
    println!(
        "MetaUtilities {} {}",
        size_of::<MetaUtilities>(),
        align_of::<MetaUtilities>()
    );
    println!("log_console {}", offset_of!(MetaUtilities, pfnLogConsole));
    println!("log_message {}", offset_of!(MetaUtilities, pfnLogMessage));
    println!(
        "user_message_id {}",
        offset_of!(MetaUtilities, pfnGetUserMsgID)
    );
    println!("log_error {}", offset_of!(MetaUtilities, pfnLogError));
    println!(
        "log_developer {}",
        offset_of!(MetaUtilities, pfnLogDeveloper)
    );
    println!(
        "DllFunctions {} {}",
        size_of::<DllFunctions>(),
        align_of::<DllFunctions>()
    );
    println!("start_frame {}", offset_of!(DllFunctions, pfnStartFrame));
    println!(
        "client_connect {}",
        offset_of!(DllFunctions, pfnClientConnect)
    );
    println!(
        "EngineFunctions {} {}",
        size_of::<EngineFunctions>(),
        align_of::<EngineFunctions>()
    );
    println!(
        "pfnServerPrint {}",
        offset_of!(EngineFunctions, pfnServerPrint)
    );
    println!(
        "pfnIndexOfEdict {}",
        offset_of!(EngineFunctions, pfnIndexOfEdict)
    );
    println!(
        "pfnServerActivate {}",
        offset_of!(DllFunctions, pfnServerActivate)
    );
    println!(
        "pfnServerDeactivate {}",
        offset_of!(DllFunctions, pfnServerDeactivate)
    );
    println!(
        "pfnClientPutInServer {}",
        offset_of!(DllFunctions, pfnClientPutInServer)
    );
    println!(
        "pfnClientDisconnect {}",
        offset_of!(DllFunctions, pfnClientDisconnect)
    );
    println!(
        "pfnCVarRegister {}",
        offset_of!(EngineFunctions, pfnCVarRegister)
    );
    println!(
        "pfnCVarGetPointer {}",
        offset_of!(EngineFunctions, pfnCVarGetPointer)
    );
    println!(
        "pfnCVarGetString {}",
        offset_of!(EngineFunctions, pfnCVarGetString)
    );
    println!(
        "pfnCVarSetString {}",
        offset_of!(EngineFunctions, pfnCVarSetString)
    );
    println!(
        "pfnAddServerCommand {}",
        offset_of!(EngineFunctions, pfnAddServerCommand)
    );
    println!("pfnCmd_Argc {}", offset_of!(EngineFunctions, pfnCmd_Argc));
    println!("pfnCmd_Argv {}", offset_of!(EngineFunctions, pfnCmd_Argv));
    println!(
        "pfnPEntityOfEntIndex {}",
        offset_of!(EngineFunctions, pfnPEntityOfEntIndex)
    );
    println!(
        "pfnGetPlayerUserId {}",
        offset_of!(EngineFunctions, pfnGetPlayerUserId)
    );
    println!(
        "pfnGetPlayerAuthId {}",
        offset_of!(EngineFunctions, pfnGetPlayerAuthId)
    );
    println!(
        "Cvar {} {}",
        size_of::<raw::cvar_t>(),
        align_of::<raw::cvar_t>()
    );
    println!("cvar_name {}", offset_of!(raw::cvar_t, name));
    println!("cvar_string {}", offset_of!(raw::cvar_t, string));
    println!("cvar_flags {}", offset_of!(raw::cvar_t, flags));
    println!("cvar_value {}", offset_of!(raw::cvar_t, value));
    println!("cvar_next {}", offset_of!(raw::cvar_t, next));
    println!(
        "pfnAllowLagCompensation {}",
        offset_of!(DllFunctions, pfnAllowLagCompensation)
    );
    println!("pfnTraceLine {}", offset_of!(EngineFunctions, pfnTraceLine));
    println!("pfnTraceHull {}", offset_of!(EngineFunctions, pfnTraceHull));
    println!("flFraction {}", offset_of!(raw::TraceResult, flFraction));
    println!("vecEndPos {}", offset_of!(raw::TraceResult, vecEndPos));
    println!(
        "vecPlaneNormal {}",
        offset_of!(raw::TraceResult, vecPlaneNormal)
    );
    println!("pHit {}", offset_of!(raw::TraceResult, pHit));
    println!("iHitgroup {}", offset_of!(raw::TraceResult, iHitgroup));
    println!("free {}", offset_of!(raw::edict_t, free));
    println!("serialnumber {}", offset_of!(raw::edict_t, serialnumber));
    println!("trace_size {}", size_of::<raw::TraceResult>());
    println!("edict_size {}", size_of::<raw::edict_t>());
    println!(
        "global_size {} {}",
        size_of::<EngineGlobals>(),
        align_of::<EngineGlobals>()
    );
    println!("global_time {}", offset_of!(EngineGlobals, time));
    println!("global_frametime {}", offset_of!(EngineGlobals, frametime));
    println!("global_mapname {}", offset_of!(EngineGlobals, mapname));
    println!("global_startspot {}", offset_of!(EngineGlobals, startspot));
    println!(
        "global_deathmatch {}",
        offset_of!(EngineGlobals, deathmatch)
    );
    println!("global_coop {}", offset_of!(EngineGlobals, coop));
    println!("global_teamplay {}", offset_of!(EngineGlobals, teamplay));
    println!(
        "global_serverflags {}",
        offset_of!(EngineGlobals, serverflags)
    );
    println!(
        "spatial_maxEntities {}",
        offset_of!(raw::globalvars_t, maxEntities)
    );
    println!(
        "spatial_maxClients {}",
        offset_of!(raw::globalvars_t, maxClients)
    );
    println!("spatial_v {}", offset_of!(raw::edict_t, v));
    println!("spatial_origin {}", offset_of!(raw::entvars_t, origin));
    println!("spatial_angles {}", offset_of!(raw::entvars_t, angles));
    println!("spatial_velocity {}", offset_of!(raw::entvars_t, velocity));
    println!("spatial_absmin {}", offset_of!(raw::entvars_t, absmin));
    println!("spatial_absmax {}", offset_of!(raw::entvars_t, absmax));
    println!("relationship_owner {}", offset_of!(raw::entvars_t, owner));
    println!("relationship_enemy {}", offset_of!(raw::entvars_t, enemy));
    println!("relationship_aiment {}", offset_of!(raw::entvars_t, aiment));
    println!(
        "relationship_groundentity {}",
        offset_of!(raw::entvars_t, groundentity)
    );
    println!("key_flags {}", offset_of!(raw::entvars_t, flags));
    println!("key_classname {}", offset_of!(raw::entvars_t, classname));
    println!("key_globalname {}", offset_of!(raw::entvars_t, globalname));
    println!("key_target {}", offset_of!(raw::entvars_t, target));
    println!("key_targetname {}", offset_of!(raw::entvars_t, targetname));
    println!("key_model {}", offset_of!(raw::entvars_t, model));
    println!("key_netname {}", offset_of!(raw::entvars_t, netname));
    println!("key_message {}", offset_of!(raw::entvars_t, message));
    println!(
        "pfnSzFromIndex {}",
        offset_of!(EngineFunctions, pfnSzFromIndex)
    );
    println!(
        "KeyValueData {} {}",
        size_of::<raw::KeyValueData>(),
        align_of::<raw::KeyValueData>()
    );
    println!("pfnKeyValue {}", offset_of!(DllFunctions, pfnKeyValue));
    println!("szClassName {}", offset_of!(raw::KeyValueData, szClassName));
    println!("szKeyName {}", offset_of!(raw::KeyValueData, szKeyName));
    println!("szValue {}", offset_of!(raw::KeyValueData, szValue));
    println!("fHandled {}", offset_of!(raw::KeyValueData, fHandled));
    println!(
        "NewDllFunctions {} {}",
        size_of::<raw::NEW_DLL_FUNCTIONS>(),
        align_of::<raw::NEW_DLL_FUNCTIONS>()
    );
    println!(
        "pfnGameShutdown {}",
        offset_of!(raw::NEW_DLL_FUNCTIONS, pfnGameShutdown)
    );
    println!("new_dll_version {}", raw::NEW_DLL_FUNCTIONS_VERSION);
    println!(
        "constants {} {} {} {} {}",
        META_INTERFACE.to_str().unwrap(),
        ENTITY_INTERFACE,
        PT_NEVER,
        PT_STARTUP,
        MRES_IGNORED
    );
}
