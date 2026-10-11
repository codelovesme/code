/* Points: 1,000 { x, y, vx, vy } moved 200 steps, bouncing off a box. */
#include <stdio.h>
typedef struct { double x, y, vx, vy; } Point;
int main(void) {
    static Point pts[1000];
    for (int i = 0; i < 1000; i++) pts[i] = (Point){ i, 1000 - i, 3, -2 };
    for (int s = 0; s < 200; s++)
        for (int i = 0; i < 1000; i++) {
            Point *p = &pts[i];
            p->x += p->vx; p->y += p->vy;
            if (p->x < 0 || p->x > 1000) p->vx = -p->vx;
            if (p->y < 0 || p->y > 1000) p->vy = -p->vy;
        }
    double sum = 0;
    for (int i = 0; i < 1000; i++) sum += pts[i].x + pts[i].y;
    printf("%.0f\n", sum);
    return sum == 1001398 ? 0 : 1;
}
