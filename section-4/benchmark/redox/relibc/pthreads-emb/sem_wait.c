/*
 * -------------------------------------------------------------
 *
 * Module: sem_wait.c
 *
 * Purpose:
 *	Semaphores aren't actually part of the PThreads standard.
 *	They are defined by the POSIX Standard:
 *
 *		POSIX 1003.1b-1993	(POSIX.1b)
 *
 * -------------------------------------------------------------
 *
 * --------------------------------------------------------------------------
 *
 *      Pthreads-embedded (PTE) - POSIX Threads Library for embedded systems
 *      Copyright(C) 2008 Jason Schmidlapp
 *
 *      Contact Email: jschmidlapp@users.sourceforge.net
 *
 *
 *      Based upon Pthreads-win32 - POSIX Threads Library for Win32
 *      Copyright(C) 1998 John E. Bossom
 *      Copyright(C) 1999,2005 Pthreads-win32 contributors
 *
 *      Contact Email: rpj@callisto.canberra.edu.au
 *
 *      The original list of contributors to the Pthreads-win32 project
 *      is contained in the file CONTRIBUTORS.ptw32 included with the
 *      source code distribution. The list can also be seen at the
 *      following World Wide Web location:
 *      http://sources.redhat.com/pthreads-win32/contributors.html
 *
 *      This library is free software; you can redistribute it and/or
 *      modify it under the terms of the GNU Lesser General Public
 *      License as published by the Free Software Foundation; either
 *      version 2 of the License, or (at your option) any later version.
 *
 *      This library is distributed in the hope that it will be useful,
 *      but WITHOUT ANY WARRANTY; without even the implied warranty of
 *      MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 *      Lesser General Public License for more details.
 *
 *      You should have received a copy of the GNU Lesser General Public
 *      License along with this library in the file COPYING.LIB;
 *      if not, write to the Free Software Foundation, Inc.,
 *      59 Temple Place - Suite 330, Boston, MA 02111-1307, USA
 */

#include <stdio.h>
#include <stdlib.h>

#include "pthread.h"
#include "semaphore.h"
#include "implement.h"


static void
pte_sem_wait_cleanup(void * sem)
{
  sem_t s = (sem_t) sem;
  unsigned int timeout;

  if (pthread_mutex_lock (&s->lock) == 0)
    {
      /*
       * If sema is destroyed do nothing, otherwise:-
       * If the sema is posted between us being cancelled and us locking
       * the sema again above then we need to consume that post but cancel
       * anyway. If we don't get the semaphore we indicate that we're no
       * longer waiting.
       */
      timeout = 0;
      if (pte_osSemaphorePend(s->sem, &timeout) != PTE_OS_OK)
        {
          ++s->value;

          /*
           * Don't release the W32 sema, it doesn't need adjustment
           * because it doesn't record the number of waiters.
           */

        }
      (void) pthread_mutex_unlock (&s->lock);
    }
}


int
sem_wait (sem_t * sem)
/*
 * ------------------------------------------------------
 * DOCPUBLIC
 *      This function  waits on a semaphore.
 *
 * PARAMETERS
 *      sem
 *              pointer to an instance of sem_t
 *
 * DESCRIPTION
 *      This function waits on a semaphore. If the
 *      semaphore value is greater than zero, it decreases
 *      its value by one. If the semaphore value is zero, then
 *      the calling thread (or process) is blocked until it can
 *      successfully decrease the value or until interrupted by
 *      a signal.
 *
 * RESULTS
 *              0               successfully decreased semaphore,
 *              -1              failed, error in errno
 * ERRNO
 *              EINVAL          'sem' is not a valid semaphore,
 *              ENOSYS          semaphores are not supported,
 *              EINTR           the function was interrupted by a signal,
 *              EDEADLK         a deadlock condition was detected.
 *
 * ------------------------------------------------------
 */
{
  int result = 0;
  sem_t s = *sem;

  pthread_testcancel();

  if (s == NULL)
    {
      result = EINVAL;
    }
  else
    {
      if ((result = pthread_mutex_lock (&s->lock)) == 0)
        {
          int v;

          /* See sem_destroy.c
           */
          if (*sem == NULL)
            {
              (void) pthread_mutex_unlock (&s->lock);
              errno = EINVAL;
              return -1;
            }

          v = --s->value;
          (void) pthread_mutex_unlock (&s->lock);

          if (v < 0)
            {
              /* Must wait */
              pthread_cleanup_push(pte_sem_wait_cleanup, (void *) s);
              result = pte_cancellable_wait(s->sem,NULL);
              /* Cleanup if we're canceled or on any other error */
              pthread_cleanup_pop(result);

              // Wait was cancelled, indicate that we're no longer waiting on this semaphore.
              /*
                      if (result == PTE_OS_INTERRUPTED)
                        {
                          result = EINTR;
                          ++s->value;
                        }
              */
            }
        }

    }

  if (result != 0)
    {
      errno = result;
      return -1;
    }

  return 0;

}				/* sem_wait */


int
sem_wait_nocancel (sem_t * sem)
/*
 * ------------------------------------------------------
 * DOCPUBLIC
 *      This function  waits on a semaphore, and doesn't
 *      allow cancellation.
 *
 * PARAMETERS
 *      sem
 *              pointer to an instance of sem_t
 *
 * DESCRIPTION
 *      This function waits on a semaphore. If the
 *      semaphore value is greater than zero, it decreases
 *      its value by one. If the semaphore value is zero, then
 *      the calling thread (or process) is blocked until it can
 *      successfully decrease the value or until interrupted by
 *      a signal.
 *
 * RESULTS
 *              0               successfully decreased semaphore,
 *              -1              failed, error in errno
 * ERRNO
 *              EINVAL          'sem' is not a valid semaphore,
 *              ENOSYS          semaphores are not supported,
 *              EINTR           the function was interrupted by a signal,
 *              EDEADLK         a deadlock condition was detected.
 *
 * ------------------------------------------------------
 */
{
  int result = 0;
  sem_t s = *sem;

  pthread_testcancel();

  if (s == NULL)
    {
      result = EINVAL;
    }
  else
    {
      if ((result = pthread_mutex_lock (&s->lock)) == 0)
        {
          int v;

          /* See sem_destroy.c
           */
          if (*sem == NULL)
            {
              (void) pthread_mutex_unlock (&s->lock);
              errno = EINVAL;
              return -1;
            }

          v = --s->value;
          (void) pthread_mutex_unlock (&s->lock);

          if (v < 0)
            {
              pte_osSemaphorePend(s->sem, NULL);
            }
        }

    }

  if (result != 0)
    {
      errno = result;
      return -1;
    }

  return 0;

}				/* sem_wait_nocancel */
