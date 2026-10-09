/* Compile/link-only fixture. Never run this fixture or use it as an application. */
#include <stddef.h>
#include <stdbool.h>
#include <SDL2/SDL_webOS.h>
#include <luna-service2/lunaservice.h>

_Static_assert(sizeof(bool) == 1, "LS2 C bool");
_Static_assert(sizeof(gboolean) == 4, "GLib gboolean");
_Static_assert(_Generic((gboolean)0, int: 1, default: 0), "gboolean is int");
_Static_assert(sizeof(LSMessageToken) == 4, "ARM32 token");
_Static_assert(sizeof(LSError) == 28 && _Alignof(LSError) == 4, "LSError ABI");
_Static_assert(offsetof(LSError, error_code) == 0, "code");
_Static_assert(offsetof(LSError, message) == 4, "message");
_Static_assert(offsetof(LSError, file) == 8, "file");
_Static_assert(offsetof(LSError, line) == 12, "line");
_Static_assert(offsetof(LSError, func) == 16, "func");
_Static_assert(offsetof(LSError, padding) == 20, "padding");
_Static_assert(offsetof(LSError, magic) == 24, "magic");

/* Assignments are ABI type assertions and retain every used LS2/GLib symbol at link. */
bool (*volatile error_init)(LSError *) = LSErrorInit;
void (*volatile error_free)(LSError *) = LSErrorFree;
bool (*volatile registration)(const char *, const char *, LSHandle **, LSError *) = LSRegisterApplicationService;
const char *(*volatile handle_name)(LSHandle *) = LSHandleGetName;
bool (*volatile attach_context)(LSHandle *, GMainContext *, LSError *) = LSGmainContextAttach;
bool (*volatile call)(LSHandle *, const char *, const char *, LSFilterFunc, void *, LSMessageToken *, LSError *) = LSCall;
bool (*volatile call_one)(LSHandle *, const char *, const char *, LSFilterFunc, void *, LSMessageToken *, LSError *) = LSCallOneReply;
bool (*volatile cancel_call)(LSHandle *, LSMessageToken, LSError *) = LSCallCancel;
bool (*volatile unregister_handle)(LSHandle *, LSError *) = LSUnregister;
const char *(*volatile sender_name)(LSMessage *) = LSMessageGetSenderServiceName;
const char *(*volatile payload)(LSMessage *) = LSMessageGetPayload;
LSMessageToken (*volatile response_token)(LSMessage *) = LSMessageGetResponseToken;
bool (*volatile hub_error)(LSMessage *) = LSMessageIsHubErrorMessage;
GMainContext *(*volatile context_new)(void) = g_main_context_new;
gboolean (*volatile context_iteration)(GMainContext *, gboolean) = g_main_context_iteration;
void (*volatile context_unref)(GMainContext *) = g_main_context_unref;
int main(void) { return 0; }
