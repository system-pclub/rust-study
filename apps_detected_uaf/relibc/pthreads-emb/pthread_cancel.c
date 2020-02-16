/*
 * pthread_cancel.c
 *
 * Description:
 * POSIX thread functions related to thread cancellation.
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

#include "pthread.h"
#include "implement.h"
#include <stdio.h>

int
pthread_cancel (pthread_t thread)
/*
 * ------------------------------------------------------
 * DOCPUBLIC
 *      This function requests cancellation of 'thread'.
 *
 * PARAMETERS
 *      thread
 *              reference to an instance of pthread_t
 *
 *
 * DESCRIPTION
 *      This function requests cancellation of 'thread'.
 *      NOTE: cancellation is asynchronous; use pthread_join to
 *                wait for termination of 'thread' if necessary.
 *
 * RESULTS
 *              0               successfully requested cancellation,
 *              ESRCH           no thread found corresponding to 'thread',
 *              ENOMEM          implicit self thread create failed.
 * ------------------------------------------------------
 */
{
  int result;
  int cancel_self;
  pthread_t self;
  pte_thread_t * tp;

  result = pthread_kill (thread, 0);

  if (0 != result)
    {
      return result;
    }

  if ((self = pthread_self ()) == NULL)
    {
      return ENOMEM;
    };

  /*
   * FIXME!!
   *
   * Can a thread cancel itself?
   *
   * The standard doesn't
   * specify an error to be returned if the target
   * thread is itself.
   *
   * If it may, then we need to ensure that a thread can't
   * deadlock itself trying to cancel itself asyncronously
   * (pthread_cancel is required to be an async-cancel
   * safe function).
   */
  cancel_self = pthread_equal (thread, self);

  tp = (pte_thread_t *) thread;

  /*
   * Lock for async-cancel safety.
   */
  (void) pthread_mutex_lock (&tp->cancelLock);

  if (tp->cancelType == PTHREAD_CANCEL_ASYNCHRONOUS
      && tp->cancelState == PTHREAD_CANCEL_ENABLE
      && tp->state < PThreadStateCanceling)
    {
      if (cancel_self)
        {
          tp->state = PThreadStateCanceling;
          tp->cancelState = PTHREAD_CANCEL_DISABLE;

          (void) pthread_mutex_unlock (&tp->cancelLock);
          pte_throw (PTE_EPS_CANCEL);

          /* Never reached */
        }
      else
        {
          /*
           * We don't support asynchronous cancellation for thread other than ourselves.
           * as it requires significant platform and OS specific functionality (see below).
           *
           * We should never get here, as we don't allow the cancellability type to be
           * sent to async.
           *
           * If you really wanted to implement async cancellation, you would probably need to
           * do something like the Win32 implement did, which is:
           *   1. Suspend the target thread.
           *   2. Replace the PC for the target thread to a routine that throws an exception
           *      or does a longjmp, depending on cleanup method.
           *   3. Resume the target thread.
           *
           * Note that most of the async cancellation code is still in here if anyone
           * wanted to add the OS/platform specific stuff.
           */
          (void) pthread_mutex_unlock (&tp->cancelLock);

          result = EPERM;

        }
    }
  else
    {
      /*
       * Set for deferred cancellation.
       */
      if (tp->state < PThreadStateCancelPending)
        {
          tp->state = PThreadStateCancelPending;

          if (pte_osThreadCancel(tp->threadId) != PTE_OS_OK)
            {
              result = ESRCH;
            }
        }
      else if (tp->state >= PThreadStateCanceling)
        {
          result = ESRCH;
        }

      (void) pthread_mutex_unlock (&tp->cancelLock);
    }

  return (result);
}
