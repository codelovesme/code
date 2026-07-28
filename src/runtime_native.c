/*
 * runtime_native.c — C bridge for native module support in compiled Code programs.
 *
 * Provides bridge functions between the compiled value format ({tag, num, ptr, bool})
 * and the native ABI CodeValue format ({tag, number, string, boolean, fields, field_count,
 * elements, element_count}).
 *
 * The compiled value_type struct layout (must match LLVM {i8, f64, i8*, i1}):
 *   offset 0:  tag      (1 byte)
 *   offset 8:  num      (8 bytes, double)  — for objects: field_count; for arrays: element_count
 *   offset 16: ptr      (8 bytes, void*)   — for strings: char*; for objects/arrays: field/element ptr
 *   offset 24: boolean  (1 byte)
 *   total: 32 bytes (with alignment padding)
 *
 * The compiled field_type struct layout (must match LLVM {i8*, value_type}):
 *   offset 0:  name     (8 bytes, char*)
 *   offset 8:  value    (32 bytes, CVal)
 *   total: 40 bytes
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <time.h>
#include <errno.h>

/* ======================================================================== */
/* Compiled value format (matches LLVM struct layout)                       */
/* ======================================================================== */

typedef struct {
    uint8_t  tag;
    double   num;
    void*    ptr;
    uint8_t  boolean;
} CVal;

typedef struct {
    const char* name;
    CVal        value;
} CField;

/* ======================================================================== */
/* Native ABI types (must match native_module.rs / code_abi.h)           */
/* ======================================================================== */

typedef struct CodeField CodeField;
typedef struct CodeValue_s CodeValue;

struct CodeValue_s {
    uint8_t            tag;
    double             number;
    const char*        string;
    uint8_t            boolean;
    const CodeField*    fields;
    uint32_t           field_count;
    const CodeValue*    elements;
    uint32_t           element_count;
};

struct CodeField {
    const char* name;
    CodeValue    value;
};

typedef CodeValue (*CodeNativeHandlerFn)(CodeValue particle);

typedef struct { const char* name; CodeValue value; }               CodeExportVar;
typedef struct { const char* class_name; CodeNativeHandlerFn handler; }       CodeExportHandler;
typedef struct { const char* name; const char* type_name; uint8_t is_optional; } CodeTypeField;
typedef struct { const char* name; const CodeTypeField* fields; uint32_t field_count; } CodeExportType;
typedef struct { const char* class_name; uint32_t target; } CodeEmission;
typedef void (*CodeEmitFn)(void* context, CodeValue particle);

typedef struct {
    uint32_t              abi_version;
    const CodeExportVar*   vars;
    uint32_t              var_count;
    const CodeExportHandler* handlers;
    uint32_t              handler_count;
    const CodeExportType*  types;
    uint32_t              type_count;
    const CodeEmission*    emissions;
    uint32_t              emission_count;
} CodeModuleDesc;

/* Tag constants. */
#define TAG_NUMBER  0
#define TAG_STRING  1
#define TAG_BOOLEAN 2
#define TAG_OBJECT  3
#define TAG_NULL    4
#define TAG_ARRAY   5

/* ======================================================================== */
/* Forward declarations                                                     */
/* ======================================================================== */

static CVal codevalue_to_cval(const CodeValue* ev);
static CodeValue cval_to_codevalue(const CVal* cv);
static void __native_register_handle(const void* desc, void* handle);
static void* __native_lookup_handle(const void* desc);

/* ======================================================================== */
/* C-level emission queue (for compiled programs)                           */
/* ======================================================================== */

typedef struct EmitNode {
    CodeValue        particle;
    struct EmitNode *next;
} EmitNode;

static pthread_mutex_t  __emit_mutex      = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t   __emit_cond       = PTHREAD_COND_INITIALIZER;
static EmitNode        *__emit_head       = NULL;
static EmitNode        *__emit_tail       = NULL;
static int              __keep_alive_flag = 0;

/*
 * C-side emit callback — invoked by native module threads.
 * Recognises the __KeepAlive sentinel (sets the keep-alive flag instead of
 * queuing it) and enqueues all other particles.
 */
static void __c_emit_enqueue(void *ctx, CodeValue particle) {
    (void)ctx;

    /* Detect __KeepAlive sentinel. */
    if (particle.tag == TAG_OBJECT) {
        for (uint32_t i = 0; i < particle.field_count; i++) {
            if (particle.fields[i].name &&
                strcmp(particle.fields[i].name, "_class") == 0 &&
                particle.fields[i].value.tag == TAG_STRING &&
                particle.fields[i].value.string &&
                strcmp(particle.fields[i].value.string, "__KeepAlive") == 0) {
                pthread_mutex_lock(&__emit_mutex);
                __keep_alive_flag = 1;
                pthread_cond_signal(&__emit_cond);
                pthread_mutex_unlock(&__emit_mutex);
                return;
            }
        }
    }

    EmitNode *node = (EmitNode *)malloc(sizeof(EmitNode));
    if (!node) return;
    node->particle = particle;
    node->next = NULL;

    pthread_mutex_lock(&__emit_mutex);
    if (__emit_tail) __emit_tail->next = node;
    else             __emit_head = node;
    __emit_tail = node;
    pthread_cond_signal(&__emit_cond);
    pthread_mutex_unlock(&__emit_mutex);
}

/* Returns 1 if the keep-alive flag was set (i.e. wait_forever was called). */
int __native_bridge_is_keep_alive(void) {
    return __keep_alive_flag;
}

/*
 * Block until an emission arrives (or a 50 ms timeout).
 *
 * If a particle is available:
 *   - Converts it to CVal and writes to *out_cval (32 bytes).
 *   - Sets *out_class_str to the _class string pointer.
 *   - Returns 1.
 *
 * If the timeout fires with nothing available, returns 0.
 */
int __native_bridge_poll_emission(void *out_cval, void **out_class_str) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_nsec += 50000000L; /* 50 ms */
    if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; }

    pthread_mutex_lock(&__emit_mutex);
    while (__emit_head == NULL) {
        int rc = pthread_cond_timedwait(&__emit_cond, &__emit_mutex, &ts);
        if (rc == ETIMEDOUT || __emit_head == NULL) {
            pthread_mutex_unlock(&__emit_mutex);
            return 0;
        }
    }

    EmitNode *node = __emit_head;
    __emit_head = node->next;
    if (!__emit_head) __emit_tail = NULL;
    pthread_mutex_unlock(&__emit_mutex);

    /* Convert to compiled CVal. */
    CVal cv = codevalue_to_cval(&node->particle);
    memcpy(out_cval, &cv, sizeof(CVal));

    /* Extract _class string pointer. */
    *out_class_str = NULL;
    if (node->particle.tag == TAG_OBJECT) {
        for (uint32_t i = 0; i < node->particle.field_count; i++) {
            if (node->particle.fields[i].name &&
                strcmp(node->particle.fields[i].name, "_class") == 0 &&
                node->particle.fields[i].value.tag == TAG_STRING) {
                *out_class_str = (void *)node->particle.fields[i].value.string;
                break;
            }
        }
    }

    free(node);
    return 1;
}



/* ======================================================================== */
/* Conversion: CodeValue → CVal (native → compiled)                         */
/* ======================================================================== */

static CVal codevalue_to_cval(const CodeValue* ev) {
    CVal cv;
    memset(&cv, 0, sizeof(cv));
    cv.tag = ev->tag;

    switch (ev->tag) {
    case TAG_NUMBER:
        cv.num = ev->number;
        break;
    case TAG_STRING:
        cv.ptr = (void*)ev->string;
        break;
    case TAG_BOOLEAN:
        cv.boolean = ev->boolean;
        break;
    case TAG_OBJECT: {
        uint32_t count = ev->field_count;
        CField* cfields = (CField*)malloc(count * sizeof(CField));
        for (uint32_t i = 0; i < count; i++) {
            cfields[i].name  = ev->fields[i].name;
            cfields[i].value = codevalue_to_cval(&ev->fields[i].value);
        }
        cv.num = (double)count;
        cv.ptr = cfields;
        break;
    }
    case TAG_ARRAY: {
        uint32_t count = ev->element_count;
        CVal* celems = (CVal*)malloc(count * sizeof(CVal));
        for (uint32_t i = 0; i < count; i++) {
            celems[i] = codevalue_to_cval(&ev->elements[i]);
        }
        cv.num = (double)count;
        cv.ptr = celems;
        break;
    }
    default: /* TAG_NULL or unknown */
        break;
    }
    return cv;
}

/* ======================================================================== */
/* Conversion: CVal → CodeValue (compiled → native)                         */
/* ======================================================================== */

static CodeValue cval_to_codevalue(const CVal* cv) {
    CodeValue ev;
    memset(&ev, 0, sizeof(ev));
    ev.tag = cv->tag;

    switch (cv->tag) {
    case TAG_NUMBER:
        ev.number = cv->num;
        break;
    case TAG_STRING:
        ev.string = (const char*)cv->ptr;
        break;
    case TAG_BOOLEAN:
        ev.boolean = cv->boolean;
        break;
    case TAG_OBJECT: {
        uint32_t count = (uint32_t)cv->num;
        const CField* cfields = (const CField*)cv->ptr;
        CodeField* efields = (CodeField*)malloc(count * sizeof(CodeField));
        for (uint32_t i = 0; i < count; i++) {
            efields[i].name  = cfields[i].name;
            efields[i].value = cval_to_codevalue(&cfields[i].value);
        }
        ev.fields      = efields;
        ev.field_count = count;
        break;
    }
    case TAG_ARRAY: {
        uint32_t count = (uint32_t)cv->num;
        const CVal* celems = (const CVal*)cv->ptr;
        CodeValue* eelems = (CodeValue*)malloc(count * sizeof(CodeValue));
        for (uint32_t i = 0; i < count; i++) {
            eelems[i] = cval_to_codevalue(&celems[i]);
        }
        ev.elements      = eelems;
        ev.element_count = count;
        break;
    }
    default: /* TAG_NULL */
        break;
    }
    return ev;
}

/* ======================================================================== */
/* Bridge functions called from compiled LLVM IR                            */
/* ======================================================================== */

/*
 * Open a native module.  Returns an opaque descriptor pointer (CodeModuleDesc*).
 * Aborts with an error message if loading fails.
 */
void* __native_bridge_open(const char* path) {
    void* handle = dlopen(path, RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "Failed to load native module '%s': %s\n", path, dlerror());
        abort();
    }

    uint32_t (*version_fn)(void) = (uint32_t(*)(void))dlsym(handle, "code_module_abi_version");
    if (!version_fn) {
        fprintf(stderr, "Native module '%s' missing 'code_module_abi_version': %s\n", path, dlerror());
        abort();
    }
    uint32_t version = version_fn();
    if (version != 2) {
        fprintf(stderr, "Native module '%s' ABI version %u (expected 2)\n", path, version);
        abort();
    }

    const CodeModuleDesc* (*init_fn)(void) = (const CodeModuleDesc*(*)(void))dlsym(handle, "code_module_init");
    if (!init_fn) {
        fprintf(stderr, "Native module '%s' missing 'code_module_init': %s\n", path, dlerror());
        abort();
    }

    const CodeModuleDesc* desc = init_fn();
    if (!desc) {
        fprintf(stderr, "Native module '%s': code_module_init() returned null\n", path);
        abort();
    }

    /* Register handle so __native_bridge_set_emit can find it later. */
    __native_register_handle(desc, handle);

    /* Auto-register the C-level emit callback for compiled programs.
     * This enables emission draining in the inline drain loop emitted by
     * codegen without any explicit codegen call to __native_bridge_set_emit. */
    void (*set_emit_fn)(CodeEmitFn, void*) =
        (void (*)(CodeEmitFn, void*))dlsym(handle, "code_module_set_emit");
    if (set_emit_fn) {
        set_emit_fn(__c_emit_enqueue, NULL);
    }

    return (void*)desc;
}

/*
 * Get the value of an exported variable at `idx`, converted to compiled format.
 * `out` must point to a CVal-sized buffer (32 bytes).
 */
void __native_bridge_get_var(void* desc, uint32_t idx, void* out) {
    const CodeModuleDesc* d = (const CodeModuleDesc*)desc;
    const CodeExportVar* var = &d->vars[idx];
    CVal cv = codevalue_to_cval(&var->value);
    memcpy(out, &cv, sizeof(CVal));
}

/*
 * Get the raw native handler function pointer at `idx`.
 */
void* __native_bridge_handler_ptr(void* desc, uint32_t idx) {
    const CodeModuleDesc* d = (const CodeModuleDesc*)desc;
    return (void*)d->handlers[idx].handler;
}

/*
 * Call a native handler.
 *   handler_ptr:  raw handler function pointer
 *   particle:     pointer to CVal (the particle in compiled format)
 *   out:          pointer to CVal buffer for the result
 */
void __native_bridge_call_handler(void* handler_ptr, const void* particle, void* out) {
    CodeNativeHandlerFn handler = (CodeNativeHandlerFn)handler_ptr;
    const CVal* cparticle = (const CVal*)particle;

    /* Convert compiled particle to native CodeValue. */
    CodeValue ev = cval_to_codevalue(cparticle);

    /* Call the native handler. */
    CodeValue result = handler(ev);

    /* Convert result back to compiled format. */
    CVal cv = codevalue_to_cval(&result);
    memcpy(out, &cv, sizeof(CVal));
}

/*
 * Set up the emit callback on a native module.
 *   desc:     opaque descriptor pointer (from __native_bridge_open)
 *   handle:   dlopen handle (stored alongside desc during open — for now
 *             we re-dlopen or pass handle)
 *
 * In the current implementation we look up `code_module_set_emit` via dlsym
 * on the same library handle.  Since __native_bridge_open only returns the
 * descriptor, we store the dlopen handle in a side table.
 */

/* Simple side-table for dlopen handles keyed by descriptor pointer. */
#define MAX_NATIVE_MODULES 64
static struct { const void* desc; void* handle; } __native_handles[MAX_NATIVE_MODULES];
static int __native_handle_count = 0;

static void __native_register_handle(const void* desc, void* handle) {
    if (__native_handle_count < MAX_NATIVE_MODULES) {
        __native_handles[__native_handle_count].desc = desc;
        __native_handles[__native_handle_count].handle = handle;
        __native_handle_count++;
    }
}

static void* __native_lookup_handle(const void* desc) {
    for (int i = 0; i < __native_handle_count; i++) {
        if (__native_handles[i].desc == desc) return __native_handles[i].handle;
    }
    return NULL;
}

/*
 * Set the emit callback for a native module.
 *   desc:    descriptor pointer (from __native_bridge_open)
 *   emit_fn: host callback function pointer
 *   context: host context pointer
 */
void __native_bridge_set_emit(void* desc, CodeEmitFn emit_fn, void* context) {
    void* handle = __native_lookup_handle(desc);
    if (!handle) return;

    void (*set_emit)(CodeEmitFn, void*) =
        (void (*)(CodeEmitFn, void*))dlsym(handle, "code_module_set_emit");
    if (set_emit) {
        set_emit(emit_fn, context);
    }
}

/* ======================================================================== */
/* String conversion helper (called from compiled code for + and interp)    */
/* ======================================================================== */

/*
 * __value_to_cstr — convert any value to a heap-allocated C string.
 *   tag: 0=Number, 1=String, 2=Boolean, 3=Object/Array, 4=Null
 *   num: numeric value (valid when tag == 0)
 *   ptr: string pointer  (valid when tag == 1)
 */
char* __value_to_cstr(int32_t tag, double num, const char* ptr) {
    char* buf;
    switch (tag) {
        case TAG_STRING:
            if (!ptr) return strdup("");
            return strdup(ptr);
        case TAG_NUMBER: {
            buf = (char*)malloc(64);
            if (!buf) return strdup("");
            /* Format as integer when value is whole, otherwise as decimal. */
            if (num == (long long)num) {
                snprintf(buf, 64, "%lld", (long long)num);
            } else {
                snprintf(buf, 64, "%g", num);
            }
            return buf;
        }
        case TAG_BOOLEAN:
            return strdup(num != 0.0 ? "true" : "false");
        default:
            return strdup("Null");
    }
}
