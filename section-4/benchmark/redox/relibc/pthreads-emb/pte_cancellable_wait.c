/*
 * pte_cancellable_wait.c
 *
 * Description:
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


int pte_cancellable_wait (pte_osSemaphoreHandle semHandle, unsigned int* timeout)
{
  int result = EINVAL;
  pte_osResult osResult;
  int cancelEnabled = 0;
  pthread_t self;
  pte_thread_t * sp;

  self = pthread_self();
  sp = (pte_thread_t *) self;

  if (sp != NULL)
    {
      /*
       * Get cancelEvent handle
       */
      if (sp->cancelState == PTHREAD_CANCEL_ENABLE)
        {
          cancelEnabled = 1;
        }
    }


  if (cancelEnabled)
    {
      osResult = pte_osSemaphoreCancellablePend(semHandle, timeout);
    }
  else
    {
      osResult = pte_osSemaphorePend(semHandle, timeout);
    }

  switch (osResult)
    {
    case PTE_OS_OK:
    {
      result = 0;
      break;
    }

    case PTE_OS_TIMEOUT:
    {
      result = ETIMEDOUT;
      break;
    }

    case PTE_OS_INTERRUPTED:
    {
      if (sp != NULL)
        {
          /*
           * Should handle POSIX and implicit POSIX threads..
           * Make sure we haven't been async-canceled in the meantime.
           */
          (void) pthread_mutex_lock (&sp->cancelLock);
          if (sp->state < PThreadStateCanceling)
            {
              sp->state = PThreadStateCanceling;
              sp->cancelState = PTHREAD_CANCEL_DISABLE;
              (void) pthread_mutex_unlock (&sp->cancelLock);
              pte_throw (PTE_EPS_CANCEL);

              /* Never reached */
            }
          (void) pthread_mutex_unlock (&sp->cancelLock);
        }
      break;
    }

    default:
    {
      result = EINVAL;
    }

    }


  return (result);

}                               /* CancelableWait */
