#include <extdll.h>
#include <meta_api.h>
#include <h_export.h>

// Give bindgen a name for the utilities' anonymous callback type.
using MetamodLog = decltype(mutil_funcs_t::pfnLogConsole);
using MetamodServerPrint = decltype(enginefuncs_t::pfnServerPrint);
using MetamodIndexOfEdict = decltype(enginefuncs_t::pfnIndexOfEdict);

#define SDK_ENGINE_CALLBACK(Name) using Metamod##Name = decltype(enginefuncs_t::pfn##Name)
SDK_ENGINE_CALLBACK(CVarRegister);
SDK_ENGINE_CALLBACK(CVarGetPointer);
SDK_ENGINE_CALLBACK(CVarGetString);
SDK_ENGINE_CALLBACK(CVarSetString);
SDK_ENGINE_CALLBACK(AddServerCommand);
SDK_ENGINE_CALLBACK(CreateFakeClient);
SDK_ENGINE_CALLBACK(ServerCommand);
SDK_ENGINE_CALLBACK(Cmd_Argc);
SDK_ENGINE_CALLBACK(Cmd_Argv);
SDK_ENGINE_CALLBACK(PEntityOfEntIndex);
SDK_ENGINE_CALLBACK(TraceLine);
SDK_ENGINE_CALLBACK(TraceHull);
SDK_ENGINE_CALLBACK(SzFromIndex);
SDK_ENGINE_CALLBACK(GetPlayerUserId);
SDK_ENGINE_CALLBACK(GetPlayerAuthId);
#undef SDK_ENGINE_CALLBACK
