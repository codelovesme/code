/* Include the chosen runtime directly, so this probes the actual implementation.
 * Compile with -I/path/to/baseline-or-candidate/src and --gc-sections. */
#include "runtime.c"
#include <assert.h>

static void check(const char *text) {
    size_t expected = 0;
    for (const unsigned char *p = (const unsigned char *)text; *p; p++)
        expected += (*p & 0xC0) != 0x80;
    CodeValue input = {0}, output = {0};
    code_str(&input, text);
    code_length_of(&output, &input);
    assert(output.tag == CODE_NUMBER && output.number == (double)expected);
    /* Output may alias an owned input: counting must finish before release. */
    code_str_owned(&input, text);
    code_length_of(&input, &input);
    assert(input.tag == CODE_NUMBER && input.number == (double)expected);
    code_release(&input);
    code_release(&output);
}

int main(void) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    char *region = mmap(NULL, page * 2, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(region != MAP_FAILED);
    assert(mprotect(region + page, page, PROT_NONE) == 0);
    /* Every alignment/tail size; the NUL is the last readable byte. Also keep
     * legacy counting for all nonzero byte patterns, not just valid UTF-8. */
    for (size_t n = 0; n <= 256; n++) {
        char *text = region + page - n - 1;
        for (unsigned int pattern = 1; pattern <= 255; pattern++) {
            memset(text, (int)pattern, n);
            text[n] = 0;
            check(text);
        }
        for (size_t i = 0; i < n; i++)
            text[i] = (char)(1 + (i * 37 + n * 13) % 255);
        text[n] = 0;
        check(text);
        if (n) {
            text[n / 2] = 0;
            check(text);
        }
    }
    assert(munmap(region, page * 2) == 0);
    return 0;
}
