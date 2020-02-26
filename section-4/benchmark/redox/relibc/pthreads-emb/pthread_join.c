/*
 * pthread_join.c
 *
 * Description:
 * This translation unit implements functions related to thread
 * synchronisation.
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

#include <pte_osal.h>

#include "pthread.h"
#include "implement.h"

#include <stdio.h>

int
pthread_join (pthread_t thread, void **value_ptr)
/*
 * ------------------------------------------------------
 * DOCPUBLIC
 *      This function waits for 'thread' to terminate and
 *      returns the thread's exit value if 'value_ptr' is not
 *      NULL. This also detaches the thread on successful
 *      completion.
 *
 * PARAMETERS
 *      thread
 *              an instance of pthread_t
 *
 *      value_ptr
 *              pointer to an instance of pointer to void
 *
 *
 * DESCRIPTION
 *      This function waits for 'thread' to terminate and
 *      returns the thread's exit value if 'value_ptr' is not
 *      NULL. This also detaches the thread on successful
 *      completion.
 *      NOTE:   detached threads cannot be joined or canceled
 *
 * RESULTS
 *              0               'thread' has completed
 *              EINVAL          thread is not a joinable thread,
 *              ESRCH           no thread could be found with ID 'thread',
 *              ENOENT          thread couldn't find it's own valid handle,
 *              EDEADLK         attempt to join thread with self
 *
 * ------------------------------------------------------
 */
{
  int result;
  pthread_t self;
  pte_thread_t * tp = (pte_thread_t *) thread;


  pte_osMutexLock (pte_thread_reuse_lock);

  if (NULL == tp)
    {
      result = ESRCH;
    }
  else if (PTHREAD_CREATE_DETACHED == tp->detachState)
    {
      result = EINVAL;
    }
  else
    {
      result = 0;
    }

  pte_osMutexUnlock(pte_thread_reuse_lock);

  if (result == 0)
    {
      /*
       * The target thread is joinable and can't be reused before we join it.
       */
      self = pthread_self();

      if (NULL == self)
        {
          result = ENOENT;
        }
      else if (pthread_equal (self, thread))
        {
          result = EDEADLK;
        }
      else
        {
          /*
           * Pthread_join is a cancelation point.
           * If we are canceled then our target thread must not be
           * detached (destroyed). This is guarranteed because
           * pthreadCancelableWait will not return if we
           * are canceled.
           */

          result = pte_osThreadWaitForEnd(tp->threadId);

          if (PTE_OS_OK == result)
            {
              if (value_ptr != NULL)
                {
                  *value_ptr = tp->exitStatus;
                }

              /*
               * The result of making multiple simultaneous calls to
               * pthread_join() or pthread_detach() specifying the same
               * target is undefined.
               */
              result = pthread_detach (thread);
            }
	  else if (result == PTE_OS_INTERRUPTED)
	    {
	      /* Call was cancelled, but still return success (per spec) */
	      result = 0;
	    }
          else
            {
              result = ESRCH;
            }
        }
    }

  return (result);

}				/* pthread_join */
