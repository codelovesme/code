#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Keep Python's resident pages out of the child's inherited RSS high-water
 * mark. Time only fork/exec/wait, excluding this measurement process's startup.
 * Leave the child's output intact and propagate its exit status. Linux reports
 * ru_maxrss in KiB: the largest resident process, including waited descendants,
 * not a sum of the process tree's simultaneously resident pages. */
int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: peak command [args ...]\n");
        return 2;
    }
    struct timespec start, end;
    if (clock_gettime(CLOCK_MONOTONIC, &start) != 0) {
        perror("clock_gettime");
        return 1;
    }
    pid_t child = fork();
    if (child < 0) {
        perror("fork");
        return 1;
    }
    if (child == 0) {
        execvp(argv[1], argv + 1);
        perror("execvp");
        _exit(127);
    }
    int status;
    struct rusage usage;
    while (wait4(child, &status, 0, &usage) < 0) {
        if (errno == EINTR) continue;
        perror("wait4");
        return 1;
    }
    if (clock_gettime(CLOCK_MONOTONIC, &end) != 0) {
        perror("clock_gettime");
        return 1;
    }
    double seconds = end.tv_sec - start.tv_sec + (end.tv_nsec - start.tv_nsec) / 1e9;
    fprintf(stderr, "\nCODE_BENCH_USAGE %.9f %ld\n", seconds, usage.ru_maxrss);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
