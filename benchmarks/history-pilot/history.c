/* Eager deep copies and explicit reference-counted sharing; ASCII text only. */
#include <assert.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#define PAYLOAD __PAYLOAD__
#define SECTIONS __SECTIONS__
#define DOCUMENT __DOCUMENT__
#define VERSIONS 64
#ifndef COW
#define COW 0
#endif
_Static_assert(PAYLOAD > VERSIONS, "document edits must be within the payload");
_Static_assert(DOCUMENT || SECTIONS == VERSIONS, "one flag edit per section");
typedef struct { size_t refs; char *bytes; } Text;
typedef struct { size_t refs; bool enabled; Text *text; } Section;
typedef struct { size_t refs; Section **items; } State;
static void *alloc(size_t n) { void *p = malloc(n); assert(p); return p; }
static Text *text_new(const char *bytes) {
    Text *t = alloc(sizeof(*t)); t->refs = 1; t->bytes = alloc(PAYLOAD);
    if (bytes) memcpy(t->bytes, bytes, PAYLOAD); else memset(t->bytes, 'a', PAYLOAD);
    return t;
}
static Text *text_hold(Text *t) { t->refs++; return t; }
static void text_drop(Text *t) { assert(t->refs); if (!--t->refs) { free(t->bytes); free(t); } }
static Section *section_new(bool enabled, Text *text) {
    Section *s = alloc(sizeof(*s)); s->refs = 1; s->enabled = enabled; s->text = text; return s;
}
static void section_drop(Section *s) {
    assert(s->refs); if (!--s->refs) { text_drop(s->text); free(s); }
}
static State *state_new(void) {
    State *s = alloc(sizeof(*s)); s->refs = 1; s->items = alloc(sizeof(*s->items) * SECTIONS); return s;
}
static State *state_copy(State *old, bool deep) {
    State *s = state_new();
    for (size_t i = 0; i < SECTIONS; i++) {
        Section *item = old->items[i];
        if (deep) s->items[i] = section_new(item->enabled, text_new(item->text->bytes));
        else { item->refs++; s->items[i] = item; }
    }
    return s;
}
static void state_drop(State *s) {
    assert(s->refs); if (!--s->refs) {
        for (size_t i = 0; i < SECTIONS; i++) section_drop(s->items[i]);
        free(s->items); free(s);
    }
}
static void edit(State **place, size_t index, size_t step) {
    State *s = *place;
#if COW
    if (s->refs > 1) { s->refs--; *place = s = state_copy(s, false); }
    Section *item = s->items[index];
    if (item->refs > 1) {
        item->refs--; s->items[index] = section_new(item->enabled, text_hold(item->text));
    }
#endif
    Section *selected = s->items[index];
    if (DOCUMENT) {
#if COW
        Text *t = selected->text;
        if (t->refs > 1) { t->refs--; selected->text = text_new(t->bytes); }
#endif
        selected->text->bytes[step] = 'b';
    } else selected->enabled = true;
}
int main(void) {
    Text *payload = text_new(NULL);
    State *current = state_new();
    for (size_t i = 0; i < SECTIONS; i++)
        current->items[i] = section_new(false, COW ? text_hold(payload) : text_new(payload->bytes));
    State *snapshots[VERSIONS + 1];
    for (size_t v = 0; v < VERSIONS; v++) {
        if (COW) { current->refs++; snapshots[v] = current; }
        else snapshots[v] = state_copy(current, true);
        edit(&current, DOCUMENT ? 0 : v, v);
    }
    snapshots[VERSIONS] = current;
    Text *expected = DOCUMENT ? text_new(payload->bytes) : text_hold(payload);
    for (size_t v = 0; v <= VERSIONS; v++) {
        for (size_t i = 0; i < SECTIONS; i++) {
            Section *item = snapshots[v]->items[i];
            assert(item->enabled == (!DOCUMENT && i < v));
            assert(item->text == expected || memcmp(item->text->bytes, expected->bytes, PAYLOAD) == 0);
        }
        if (DOCUMENT && v < VERSIONS) expected->bytes[v] = 'b';
    }
    for (size_t v = 0; v <= VERSIONS; v++) state_drop(snapshots[v]);
    text_drop(expected); text_drop(payload);
    return 0;
}
