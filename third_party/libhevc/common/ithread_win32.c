/*
 * Windows implementation of ithread.h for libhevc.
 * Replaces the POSIX-only ithread.c on Win32.
 * Apache-2.0 (same license as libhevc)
 */
#include <string.h>
#include <stdlib.h>
#include "ihevc_typedefs.h"
#include "ithread.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <process.h>

/* --- Thread --- */

typedef struct {
    HANDLE handle;
} ithread_win32_t;

typedef struct {
    void *(*start_routine)(void *);
    void *arg;
} ithread_win32_start_t;

static unsigned __stdcall ithread_win32_trampoline(void *p)
{
    ithread_win32_start_t ctx = *(ithread_win32_start_t *)p;
    free(p);
    ctx.start_routine(ctx.arg);
    return 0;
}

UWORD32 ithread_get_handle_size(void)
{
    return sizeof(ithread_win32_t);
}

WORD32 ithread_create(void *thread_handle, void *attribute, void *strt, void *argument)
{
    (void)attribute;
    ithread_win32_start_t *ctx = (ithread_win32_start_t *)malloc(sizeof(*ctx));
    if(!ctx) return -1;
    ctx->start_routine = (void *(*)(void *))strt;
    ctx->arg = argument;
    ithread_win32_t *t = (ithread_win32_t *)thread_handle;
    t->handle = (HANDLE)_beginthreadex(NULL, 0, ithread_win32_trampoline, ctx, 0, NULL);
    if(!t->handle) { free(ctx); return -1; }
    return 0;
}

WORD32 ithread_join(void *thread_handle, void **val_ptr)
{
    (void)val_ptr;
    ithread_win32_t *t = (ithread_win32_t *)thread_handle;
    WaitForSingleObject(t->handle, INFINITE);
    CloseHandle(t->handle);
    t->handle = NULL;
    return 0;
}

void ithread_exit(void *val_ptr)
{
    (void)val_ptr;
    _endthreadex(0);
}

/* --- Mutex (CRITICAL_SECTION) --- */

UWORD32 ithread_get_mutex_lock_size(void)
{
    return sizeof(CRITICAL_SECTION);
}

WORD32 ithread_get_mutex_struct_size(void)
{
    return sizeof(CRITICAL_SECTION);
}

WORD32 ithread_mutex_init(void *mutex)
{
    InitializeCriticalSection((CRITICAL_SECTION *)mutex);
    return 0;
}

WORD32 ithread_mutex_destroy(void *mutex)
{
    DeleteCriticalSection((CRITICAL_SECTION *)mutex);
    return 0;
}

WORD32 ithread_mutex_lock(void *mutex)
{
    EnterCriticalSection((CRITICAL_SECTION *)mutex);
    return 0;
}

WORD32 ithread_mutex_unlock(void *mutex)
{
    LeaveCriticalSection((CRITICAL_SECTION *)mutex);
    return 0;
}

/* --- Yield / Sleep --- */

void ithread_yield(void)
{
    SwitchToThread();
}

void ithread_sleep(UWORD32 u4_time)
{
    Sleep(u4_time * 1000);
}

void ithread_msleep(UWORD32 u4_time_ms)
{
    Sleep(u4_time_ms);
}

void ithread_usleep(UWORD32 u4_time_us)
{
    UWORD32 ms = u4_time_us / 1000;
    if(ms == 0) ms = 1;
    Sleep(ms);
}

/* --- Semaphore (Win32 kernel object) --- */

UWORD32 ithread_get_sem_struct_size(void)
{
    return sizeof(HANDLE);
}

WORD32 ithread_sem_init(void *sem, WORD32 pshared, UWORD32 value)
{
    (void)pshared;
    HANDLE *h = (HANDLE *)sem;
    *h = CreateSemaphoreA(NULL, (LONG)value, 0x7FFFFFFF, NULL);
    return *h ? 0 : -1;
}

WORD32 ithread_sem_post(void *sem)
{
    return ReleaseSemaphore(*(HANDLE *)sem, 1, NULL) ? 0 : -1;
}

WORD32 ithread_sem_wait(void *sem)
{
    return WaitForSingleObject(*(HANDLE *)sem, INFINITE) == WAIT_OBJECT_0 ? 0 : -1;
}

WORD32 ithread_sem_destroy(void *sem)
{
    CloseHandle(*(HANDLE *)sem);
    return 0;
}

/* --- Affinity --- */

WORD32 ithread_set_affinity(WORD32 core_id)
{
    DWORD_PTR mask = (DWORD_PTR)1 << core_id;
    if(SetThreadAffinityMask(GetCurrentThread(), mask))
        return core_id;
    return -1;
}

/* --- Condition Variable (Vista+) --- */

WORD32 ithread_get_cond_struct_size(void)
{
    return sizeof(CONDITION_VARIABLE);
}

WORD32 ithread_cond_init(void *cond)
{
    InitializeConditionVariable((CONDITION_VARIABLE *)cond);
    return 0;
}

WORD32 ithread_cond_destroy(void *cond)
{
    (void)cond;
    return 0;
}

WORD32 ithread_cond_wait(void *cond, void *mutex)
{
    return SleepConditionVariableCS(
        (CONDITION_VARIABLE *)cond,
        (CRITICAL_SECTION *)mutex,
        INFINITE) ? 0 : -1;
}

WORD32 ithread_cond_signal(void *cond)
{
    WakeConditionVariable((CONDITION_VARIABLE *)cond);
    return 0;
}
