/*
 * POSIX implementation of ithread.h for libhevc.
 * Apache-2.0 (same license as libhevc)
 */
#include <pthread.h>
#include <sched.h>
#include <stdlib.h>
#include <unistd.h>

#include "ihevc_typedefs.h"
#include "ithread.h"

typedef struct
{
    UWORD32 count;
    pthread_mutex_t mutex;
    pthread_cond_t cond;
} ithread_posix_sem_t;

UWORD32 ithread_get_handle_size(void)
{
    return sizeof(pthread_t);
}

WORD32 ithread_create(void *thread_handle, void *attribute, void *strt, void *argument)
{
    (void)attribute;
    return pthread_create((pthread_t *)thread_handle, NULL, (void *(*)(void *))strt, argument);
}

WORD32 ithread_join(void *thread_handle, void **val_ptr)
{
    return pthread_join(*(pthread_t *)thread_handle, val_ptr);
}

void ithread_exit(void *val_ptr)
{
    pthread_exit(val_ptr);
}

UWORD32 ithread_get_mutex_lock_size(void)
{
    return sizeof(pthread_mutex_t);
}

WORD32 ithread_get_mutex_struct_size(void)
{
    return sizeof(pthread_mutex_t);
}

WORD32 ithread_mutex_init(void *mutex)
{
    return pthread_mutex_init((pthread_mutex_t *)mutex, NULL);
}

WORD32 ithread_mutex_destroy(void *mutex)
{
    return pthread_mutex_destroy((pthread_mutex_t *)mutex);
}

WORD32 ithread_mutex_lock(void *mutex)
{
    return pthread_mutex_lock((pthread_mutex_t *)mutex);
}

WORD32 ithread_mutex_unlock(void *mutex)
{
    return pthread_mutex_unlock((pthread_mutex_t *)mutex);
}

void ithread_yield(void)
{
    sched_yield();
}

void ithread_sleep(UWORD32 u4_time)
{
    sleep(u4_time);
}

void ithread_msleep(UWORD32 u4_time_ms)
{
    usleep(u4_time_ms * 1000);
}

void ithread_usleep(UWORD32 u4_time_us)
{
    usleep(u4_time_us);
}

UWORD32 ithread_get_sem_struct_size(void)
{
    return sizeof(ithread_posix_sem_t);
}

WORD32 ithread_sem_init(void *sem, WORD32 pshared, UWORD32 value)
{
    (void)pshared;
    ithread_posix_sem_t *s = (ithread_posix_sem_t *)sem;
    s->count = value;
    if(pthread_mutex_init(&s->mutex, NULL) != 0) return -1;
    if(pthread_cond_init(&s->cond, NULL) != 0)
    {
        pthread_mutex_destroy(&s->mutex);
        return -1;
    }
    return 0;
}

WORD32 ithread_sem_post(void *sem)
{
    ithread_posix_sem_t *s = (ithread_posix_sem_t *)sem;
    pthread_mutex_lock(&s->mutex);
    s->count++;
    pthread_cond_signal(&s->cond);
    pthread_mutex_unlock(&s->mutex);
    return 0;
}

WORD32 ithread_sem_wait(void *sem)
{
    ithread_posix_sem_t *s = (ithread_posix_sem_t *)sem;
    pthread_mutex_lock(&s->mutex);
    while(s->count == 0)
    {
        pthread_cond_wait(&s->cond, &s->mutex);
    }
    s->count--;
    pthread_mutex_unlock(&s->mutex);
    return 0;
}

WORD32 ithread_sem_destroy(void *sem)
{
    ithread_posix_sem_t *s = (ithread_posix_sem_t *)sem;
    pthread_cond_destroy(&s->cond);
    pthread_mutex_destroy(&s->mutex);
    return 0;
}

WORD32 ithread_set_affinity(WORD32 core_id)
{
    return core_id;
}

WORD32 ithread_get_cond_struct_size(void)
{
    return sizeof(pthread_cond_t);
}

WORD32 ithread_cond_init(void *cond)
{
    return pthread_cond_init((pthread_cond_t *)cond, NULL);
}

WORD32 ithread_cond_destroy(void *cond)
{
    return pthread_cond_destroy((pthread_cond_t *)cond);
}

WORD32 ithread_cond_wait(void *cond, void *mutex)
{
    return pthread_cond_wait((pthread_cond_t *)cond, (pthread_mutex_t *)mutex);
}

WORD32 ithread_cond_signal(void *cond)
{
    return pthread_cond_signal((pthread_cond_t *)cond);
}
