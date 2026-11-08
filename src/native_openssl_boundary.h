/* Exact native source boundary for OpenSSL 3.6.3. It exports private app
 * objects and calls; no pthread structure or symbol crosses the kernel ABI.
 */
#if defined(OPENSSL_SYS_TRUEOS) && !defined(__ASSEMBLER__)
#ifndef OSSL_TRUEOS_NATIVE_H
#define OSSL_TRUEOS_NATIVE_H
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <unistd.h>
#include <sys/time.h>
#include <time.h>

typedef struct { void *object; } OsslTrueosMutex;
typedef struct { void *object; } OsslTrueosCond;
typedef struct { int kind; } OsslTrueosMutexAttr;
int ossl_trueos_mutex_init(OsslTrueosMutex *, const OsslTrueosMutexAttr *);
int ossl_trueos_mutex_destroy(OsslTrueosMutex *);
int ossl_trueos_mutex_lock(OsslTrueosMutex *);
int ossl_trueos_mutex_unlock(OsslTrueosMutex *);
int ossl_trueos_mutexattr_init(OsslTrueosMutexAttr *);
int ossl_trueos_mutexattr_destroy(OsslTrueosMutexAttr *);
int ossl_trueos_mutexattr_settype(OsslTrueosMutexAttr *, int);
int ossl_trueos_cond_init(OsslTrueosCond *, const void *);
int ossl_trueos_cond_destroy(OsslTrueosCond *);
int ossl_trueos_cond_wait(OsslTrueosCond *, OsslTrueosMutex *);
int ossl_trueos_cond_signal(OsslTrueosCond *);
int ossl_trueos_cond_broadcast(OsslTrueosCond *);
int ossl_trueos_key_create(uint32_t *, void (*)(void *));
int ossl_trueos_key_delete(uint32_t);
void *ossl_trueos_getspecific(uint32_t);
int ossl_trueos_setspecific(uint32_t, const void *);
uintptr_t ossl_trueos_thread_self(void);
int *ossl_trueos_errno(void);
int ossl_trueos_yield(void);
int ossl_trueos_getpid(void);
char *ossl_trueos_getenv(const char *);
time_t ossl_trueos_time(time_t *);
int ossl_trueos_gettimeofday(struct timeval *, void *);
int ossl_trueos_clock_gettime(clockid_t, struct timespec *);
int ossl_trueos_nanosleep(const struct timespec *, struct timespec *);
int ossl_trueos_strerror_r(int, char *, size_t);
int ossl_trueos_getentropy(void *, size_t);
void *trueos_blueprint_c_malloc(size_t);
void *trueos_blueprint_c_calloc(size_t, size_t);
void *trueos_blueprint_c_realloc(void *, size_t);
void trueos_blueprint_c_free(void *);

#define pthread_mutex_t OsslTrueosMutex
#define pthread_cond_t OsslTrueosCond
#define pthread_mutexattr_t OsslTrueosMutexAttr
#define pthread_t uintptr_t
#define pthread_key_t uint32_t
#undef PTHREAD_MUTEX_INITIALIZER
#define PTHREAD_MUTEX_INITIALIZER {NULL}
#undef PTHREAD_COND_INITIALIZER
#define PTHREAD_COND_INITIALIZER {NULL}
#define pthread_mutex_init ossl_trueos_mutex_init
#define pthread_mutex_destroy ossl_trueos_mutex_destroy
#define pthread_mutex_lock ossl_trueos_mutex_lock
#define pthread_mutex_unlock ossl_trueos_mutex_unlock
#define pthread_mutexattr_init ossl_trueos_mutexattr_init
#define pthread_mutexattr_destroy ossl_trueos_mutexattr_destroy
#define pthread_mutexattr_settype ossl_trueos_mutexattr_settype
#define pthread_cond_init ossl_trueos_cond_init
#define pthread_cond_destroy ossl_trueos_cond_destroy
#define pthread_cond_wait ossl_trueos_cond_wait
#define pthread_cond_signal ossl_trueos_cond_signal
#define pthread_cond_broadcast ossl_trueos_cond_broadcast
#define pthread_key_create ossl_trueos_key_create
#define pthread_key_delete ossl_trueos_key_delete
#define pthread_getspecific ossl_trueos_getspecific
#define pthread_setspecific ossl_trueos_setspecific
#define pthread_self ossl_trueos_thread_self
#undef errno
#define errno (*ossl_trueos_errno())
#define sched_yield ossl_trueos_yield
#define getpid ossl_trueos_getpid
#define getenv ossl_trueos_getenv
#define time(value) ossl_trueos_time(value)
#define gettimeofday ossl_trueos_gettimeofday
#define clock_gettime ossl_trueos_clock_gettime
#define nanosleep ossl_trueos_nanosleep
#define malloc trueos_blueprint_c_malloc
#define calloc trueos_blueprint_c_calloc
#define realloc trueos_blueprint_c_realloc
#define free trueos_blueprint_c_free
#endif
#endif
