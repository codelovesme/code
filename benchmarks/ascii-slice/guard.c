/* Actual selected runtime, including the native ASCII helper. */
#include "runtime.c"
#include <assert.h>

static size_t reference_offset(const char *s, size_t limit) {
    size_t i = 0, seen = 0;
    while (s[i] && seen < limit) {
        i++;
        while ((s[i] & 0xC0) == 0x80) i++;
        seen++;
    }
    return i;
}

static void check(const char *text, double lower, double upper) {
    size_t limit = (size_t)-1;
    size_t lo = lower >= (double)limit ? limit : (size_t)lower;
    size_t hi = upper >= (double)limit ? limit : (size_t)upper;
    size_t begin = lo >= hi ? 0 : reference_offset(text, lo);
    size_t end = lo >= hi ? 0 : reference_offset(text, hi);
    size_t count = end - begin;
    CodeValue input = {0}, output = {0}, from = {0}, to = {0};
    code_str(&input, text);
    code_number(&from, lower);
    code_number(&to, upper);
    code_slice(&output, &input, &from, &to);
    assert(output.tag == CODE_STR && strlen(output.str) == count);
    assert(memcmp(output.str, text + begin, count) == 0);
    code_str_owned(&input, text);
    code_slice(&input, &input, &from, &to);
    assert(input.tag == CODE_STR && strlen(input.str) == count);
    assert(memcmp(input.str, text + begin, count) == 0);
    code_release(&input);
    code_release(&output);
}

int main(void) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    char *region = mmap(NULL, page * 2, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(region != MAP_FAILED);
    assert(mprotect(region + page, page, PROT_NONE) == 0);
    const double bounds[] = {0, 1, 7, 8, 255, 256, 257, 1e200};
    const size_t transitions[] = {0, 7, 8, 63, 64, 255, 256, 257, 10000};
    for (size_t n = 0; n <= 529; n++) {
        char *text = region + page - n - 1;
        for (size_t k = 0; k < sizeof(transitions) / sizeof(*transitions); k++) {
            for (size_t i = 0; i < n; i++)
                text[i] = i < transitions[k] ? 'a' :
                    ((i - transitions[k]) % 2 ? (char)0xa9 : (char)0xc3);
            text[n] = 0;
            for (size_t a = 0; a < sizeof(bounds) / sizeof(*bounds); a++)
                for (size_t b = 0; b < sizeof(bounds) / sizeof(*bounds); b++)
                    check(text, bounds[a], bounds[b]);
        }
    }
    assert(munmap(region, page * 2) == 0);
    return 0;
}
