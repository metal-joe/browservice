/* Test helper for the PANIC path of the Rust port (not part of any build).
 *
 * A vice plugin shim that is linked against retrojsvice.so and only overrides
 * vicePluginAPI_start: it forwards the call to retrojsvice and starts a thread
 * that, a few seconds later (when CEF has been initialized), calls the
 * closeWindow callback outside vicePluginAPI_pumpEvents. The program must
 * detect this misuse and end through its PANIC path. All other API functions
 * are resolved from retrojsvice.so through this library's dependency.
 *
 * Build and use (in the directory containing browservice and retrojsvice.so):
 *   gcc -shared -fPIC -O2 -I. tools/rust_port_panic_test_plugin.c \
 *       -o target/release/panicvice.so -Ltarget/release \
 *       -Wl,--no-as-needed -l:retrojsvice.so \
 *       -Wl,-rpath,'$ORIGIN' -lpthread -ldl
 *   ./browservice --vice-plugin=panicvice.so ...
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "vice_plugin_api.h"

static VicePluginAPI_Callbacks savedCallbacks;
static void* savedCallbackData;

static void* misbehave(void* arg) {
    (void)arg;
    const char* delayStr = getenv("PANIC_TEST_DELAY");
    sleep(delayStr != NULL ? (unsigned)atoi(delayStr) : 5);
    fprintf(stderr, "panic test plugin: calling closeWindow outside pumpEvents\n");
    fflush(stderr);
    savedCallbacks.closeWindow(savedCallbackData, 1);
    return NULL;
}

void vicePluginAPI_start(
    VicePluginAPI_Context* ctx,
    VicePluginAPI_Callbacks callbacks,
    void* callbackData
) {
    void (*realStart)(VicePluginAPI_Context*, VicePluginAPI_Callbacks, void*) =
        (void (*)(VicePluginAPI_Context*, VicePluginAPI_Callbacks, void*))
            dlsym(RTLD_NEXT, "vicePluginAPI_start");
    if(realStart == NULL) {
        fprintf(stderr, "panic test plugin: vicePluginAPI_start not found\n");
        abort();
    }
    realStart(ctx, callbacks, callbackData);

    savedCallbacks = callbacks;
    savedCallbackData = callbackData;
    pthread_t thread;
    pthread_create(&thread, NULL, misbehave, NULL);
    pthread_detach(thread);
}
