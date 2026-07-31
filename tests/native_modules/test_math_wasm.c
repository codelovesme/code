/*
 * test_math_wasm.c — Code WASM native module for testing.
 *
 * Implements the same interface as test_math.c but using the Code WASM ABI
 * (linear-memory layout, wasm32 pointers).
 *
 * Exports:
 *   Variables:  PI (Number)
 *   Types:      Point { x: Number, y: Number }
 *   Handlers:   Point (returns particle + sum field)  (exported as code_handler_0)
 *
 * Build with:
 *   clang --target=wasm32 -nostdlib -O2 \
 *     -Wl,--no-entry -Wl,--export-all \
 *     -o test_math.wasm test_math_wasm.c
 */

/* ---- Minimal runtime (no libc in wasm32 -nostdlib) ---- */

typedef unsigned char      uint8_t;
typedef unsigned int       uint32_t;
typedef unsigned long long uint64_t;
typedef signed int         int32_t;
/* In wasm32, pointers are 32-bit. */
typedef uint32_t           uintptr_t;

/* Simple bump allocator backed by wasm memory. */
static uint8_t heap[65536];
static uint32_t heap_top = 0;

void *code_alloc_raw(uint32_t size) {
    /* Align to 8 bytes. */
    uint32_t aligned = (size + 7) & ~7u;
    void *ptr = heap + heap_top;
    heap_top += aligned;
    return ptr;
}

/* code_alloc: exported, takes size, returns offset into linear memory. */
__attribute__((export_name("code_alloc")))
int32_t code_alloc(int32_t size) {
    /* heap is a static array; its address IS its linear-memory offset. */
    uint32_t off = (uint32_t)(uintptr_t)(heap + heap_top);
    uint32_t aligned = ((uint32_t)size + 7) & ~7u;
    heap_top += aligned;
    return (int32_t)off;
}

/* Minimal memcpy (needed for string ops). */
static void *my_memcpy(void *dst, const void *src, uint32_t n) {
    uint8_t *d = dst;
    const uint8_t *s = src;
    for (uint32_t i = 0; i < n; i++) d[i] = s[i];
    return dst;
}

static uint32_t my_strlen(const char *s) {
    uint32_t n = 0;
    while (s[n]) n++;
    return n;
}

/* ---- CodeValue memory layout (32 bytes, matches wasm_module.rs) ---- */
/*
 * offset  size  field
 * 0       1     tag
 * 1       7     pad
 * 8       8     number (double / f64)
 * 16      4     ptr    (u32, string/fields/elements offset)
 * 20      4     count  (u32, field_count / element_count)
 * 24      1     boolean
 * 25      7     pad
 */
#define CODE_TAG_NUMBER  0
#define CODE_TAG_STRING  1
#define CODE_TAG_BOOLEAN 2
#define CODE_TAG_OBJECT  3
#define CODE_TAG_NULL    4
#define CODE_TAG_ARRAY   5

#define VAL_SIZE  32
#define VAL_TAG    0
#define VAL_NUM    8
#define VAL_PTR   16
#define VAL_COUNT 20
#define VAL_BOOL  24

/* CodeField (40 bytes):
 *   0   4   name_ptr  (u32)
 *   4   4   pad
 *   8  32   CodeValue
 */
#define FIELD_SIZE   40
#define FIELD_NAME    0
#define FIELD_VALUE   8

typedef struct {
    uint8_t bytes[VAL_SIZE];
} CodeVal;

typedef struct {
    uint8_t bytes[FIELD_SIZE];
} CodeField;

/* Read / write helpers (little-endian). */
static void write_u32(uint8_t *p, uint32_t v) {
    p[0] = v & 0xff; p[1] = (v >> 8) & 0xff;
    p[2] = (v >> 16) & 0xff; p[3] = (v >> 24) & 0xff;
}

static uint32_t read_u32(const uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8)
         | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void write_f64(uint8_t *p, double v) {
    uint8_t tmp[8];
    my_memcpy(tmp, &v, 8);
    my_memcpy(p, tmp, 8);
}

static double read_f64(const uint8_t *p) {
    double v;
    my_memcpy(&v, p, 8);
    return v;
}

/* Build a Number CodeVal. */
static CodeVal make_number(double n) {
    CodeVal v = {{0}};
    v.bytes[VAL_TAG] = CODE_TAG_NUMBER;
    write_f64(v.bytes + VAL_NUM, n);
    return v;
}

/* Build a String CodeVal (allocates cstring in linear memory). */
static CodeVal make_string(const char *s) {
    uint32_t len = my_strlen(s) + 1;
    int32_t ptr = code_alloc(len);
    my_memcpy((void *)(uintptr_t)(uint32_t)ptr, s, len);
    CodeVal v = {{0}};
    v.bytes[VAL_TAG] = CODE_TAG_STRING;
    write_u32(v.bytes + VAL_PTR, (uint32_t)ptr);
    return v;
}

/* Build a Null CodeVal. */
static CodeVal make_null(void) {
    CodeVal v = {{0}};
    v.bytes[VAL_TAG] = CODE_TAG_NULL;
    return v;
}

/* Write CodeVal bytes into memory at offset. */
static void write_val(int32_t off, CodeVal v) {
    my_memcpy((void *)(uintptr_t)(uint32_t)off, v.bytes, VAL_SIZE);
}

/* Read CodeVal bytes from memory at offset. */
static CodeVal read_val(int32_t off) {
    CodeVal v;
    my_memcpy(v.bytes, (void *)(uintptr_t)(uint32_t)off, VAL_SIZE);
    return v;
}

/* ---- Handler ---- */

/* code_handler_0: handle_point(particle) -> Object */
__attribute__((export_name("code_handler_0")))
void code_handler_point(int32_t particle_ptr, int32_t result_ptr) {
    CodeVal particle = read_val(particle_ptr);
    if (particle.bytes[VAL_TAG] != CODE_TAG_OBJECT) {
        write_val(result_ptr, make_null()); return;
    }
    uint32_t fields_ptr = read_u32(particle.bytes + VAL_PTR);
    uint32_t field_count = read_u32(particle.bytes + VAL_COUNT);

    double x = 0.0, y = 0.0;
    for (uint32_t i = 0; i < field_count; i++) {
        uint32_t field_off = fields_ptr + i * FIELD_SIZE;
        CodeField f;
        my_memcpy(f.bytes, (void *)(uintptr_t)field_off, FIELD_SIZE);
        uint32_t name_ptr = read_u32(f.bytes + FIELD_NAME);
        const char *fname = (const char *)(uintptr_t)name_ptr;
        CodeVal fval;
        my_memcpy(fval.bytes, f.bytes + FIELD_VALUE, VAL_SIZE);
        if (fval.bytes[VAL_TAG] == CODE_TAG_NUMBER) {
            /* Simple string compare without libc. */
            const char *sx = "x", *sy = "y";
            /* compare fname vs "x" */
            if (fname[0] == sx[0] && fname[1] == '\0') x = read_f64(fval.bytes + VAL_NUM);
            if (fname[0] == sy[0] && fname[1] == '\0') y = read_f64(fval.bytes + VAL_NUM);
        }
    }

    /* Build result object: { x, y, sum } — 3 fields */
    uint32_t n_fields = 3;
    int32_t new_fields_off = code_alloc((int32_t)(n_fields * FIELD_SIZE));

    /* Helper lambda: write one field at index i. */
    #define WRITE_FIELD(idx, name_str, val_expr) do { \
        uint32_t fo = (uint32_t)new_fields_off + (idx) * FIELD_SIZE; \
        const char *fn_str = (name_str); \
        uint32_t fn_len = my_strlen(fn_str) + 1; \
        int32_t fn_off = code_alloc((int32_t)fn_len); \
        my_memcpy((void *)(uintptr_t)(uint32_t)fn_off, fn_str, fn_len); \
        uint8_t *fp = (uint8_t *)(uintptr_t)fo; \
        write_u32(fp + FIELD_NAME, (uint32_t)fn_off); \
        CodeVal fv_val = (val_expr); \
        my_memcpy(fp + FIELD_VALUE, fv_val.bytes, VAL_SIZE); \
    } while(0)

    WRITE_FIELD(0, "x",   make_number(x));
    WRITE_FIELD(1, "y",   make_number(y));
    WRITE_FIELD(2, "sum", make_number(x + y));
    #undef WRITE_FIELD

    CodeVal result = {{0}};
    result.bytes[VAL_TAG] = CODE_TAG_OBJECT;
    write_u32(result.bytes + VAL_PTR,   (uint32_t)new_fields_off);
    write_u32(result.bytes + VAL_COUNT, n_fields);
    write_val(result_ptr, result);
}

/* ---- Module descriptor in linear memory ---- */

/*
 * CodeModuleDesc layout (44 bytes, stored as static array):
 *   [0]  abi_version    u32
 *   [4]  vars_ptr       u32
 *   [8]  var_count      u32
 *   [12] reserved       u32  (must be 0 — Code has no function-call concept)
 *   [16] reserved       u32  (must be 0)
 *   [20] handlers_ptr   u32
 *   [24] handler_count  u32
 *   [28] types_ptr      u32
 *   [32] type_count     u32
 *   [36] emissions_ptr  u32
 *   [40] emission_count u32
 *
 * CodeExportVar = 40 bytes: name_ptr(4) + pad(4) + CodeValue(32)
 * CodeExportHandler = 8 bytes: class_name_ptr(4) + func_idx(4)
 * CodeExportType = 12 bytes: name_ptr(4) + fields_ptr(4) + field_count(4)
 * CodeTypeField  = 12 bytes: name_ptr(4) + type_name_ptr(4) + is_optional(1) + pad(3)
 */

/* Static string literals for the descriptor. */
static const char str_PI[]      = "PI";
static const char str_Point[]   = "Point";
static const char str_x[]       = "x";
static const char str_y[]       = "y";
static const char str_Number[]  = "Number";

/* CodeExportVar: PI = 3.14159265358979 */
/* 40 bytes: name_ptr(4) + pad(4) + CodeValue(32) */
static const uint8_t var_PI[40] = {0};  /* filled at init */

/* CodeExportHandler (8 bytes). */
static uint8_t handler_Point[8] = {0};

/* CodeTypeField entries for Point type — stored contiguously. */
static uint8_t type_fields_Point[2 * 12];  /* 2 fields × 12 bytes */
#define tf_x (type_fields_Point + 0)
#define tf_y (type_fields_Point + 12)

/* CodeExportType for Point. */
static uint8_t type_Point[12] = {0};

/* Module descriptor. */
static uint8_t module_desc[44] = {0};

/* Exported mutable var array (allocate on heap so we can hold CodeValue). */
static uint8_t var_PI_mem[40];

/*
 * code_module_init: initialise the descriptor and return its offset.
 */
__attribute__((export_name("code_module_init")))
int32_t code_module_init_fn(void) {
    /* --- Build variable: PI --- */
    /* name_ptr = &str_PI */
    uint32_t pi_name_ptr = (uint32_t)(uintptr_t)str_PI;
    write_u32(var_PI_mem + 0, pi_name_ptr);   /* name_ptr */
    /* pad 4 bytes at [4] */
    /* CodeValue at [8]: tag=NUMBER, number=3.14159265358979 */
    var_PI_mem[8 + 0] = CODE_TAG_NUMBER;       /* tag at [8+0] */
    write_f64(var_PI_mem + 8 + 8, 3.14159265358979); /* number at [8+8] */

    /* --- Build handler: Point (func_idx=0) --- */
    write_u32(handler_Point + 0, (uint32_t)(uintptr_t)str_Point);
    write_u32(handler_Point + 4, 0);  /* func_idx */

    /* --- Build type fields for Point --- */
    write_u32(tf_x + 0, (uint32_t)(uintptr_t)str_x);
    write_u32(tf_x + 4, (uint32_t)(uintptr_t)str_Number);
    tf_x[8] = 0;  /* not optional */

    write_u32(tf_y + 0, (uint32_t)(uintptr_t)str_y);
    write_u32(tf_y + 4, (uint32_t)(uintptr_t)str_Number);
    tf_y[8] = 0;

    /* type_Point: name_ptr, fields_ptr (=&tf_x[0]), field_count=2 */
    write_u32(type_Point + 0, (uint32_t)(uintptr_t)str_Point);
    /* fields_ptr: address of first CodeTypeField (tf_x is first) */
    write_u32(type_Point + 4, (uint32_t)(uintptr_t)tf_x);
    write_u32(type_Point + 8, 2);

    /* --- Build module descriptor --- */
    write_u32(module_desc + 0,  2);                              /* abi_version */
    write_u32(module_desc + 4,  (uint32_t)(uintptr_t)var_PI_mem); /* vars_ptr */
    write_u32(module_desc + 8,  1);                              /* var_count */
    write_u32(module_desc + 12, 0);                              /* reserved, must be 0 */
    write_u32(module_desc + 16, 0);                              /* reserved, must be 0 */
    write_u32(module_desc + 20, (uint32_t)(uintptr_t)handler_Point); /* handlers_ptr */
    write_u32(module_desc + 24, 1);                              /* handler_count */
    write_u32(module_desc + 28, (uint32_t)(uintptr_t)type_Point); /* types_ptr */
    write_u32(module_desc + 32, 1);                              /* type_count */
    write_u32(module_desc + 36, 0);                              /* emissions_ptr (none) */
    write_u32(module_desc + 40, 0);                              /* emission_count */

    return (int32_t)(uint32_t)(uintptr_t)module_desc;
}

__attribute__((export_name("code_module_abi_version")))
int32_t code_module_abi_version_fn(void) {
    return 2;
}
