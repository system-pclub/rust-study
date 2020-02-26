/*
 * pthread_mutex_timedlock.c
 *
 * Description:
 * This translation unit implements mutual exclusion (mutex) primitives.
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

#include <stdio.h>
#include <stdlib.h>

#include "pthread.h"
#include "implement.h"


static int
pte_timed_eventwait (pte_osSemaphoreHandle event, const struct timespec *abstime)
/*
 * ------------------------------------------------------
 * DESCRIPTION
 *      This function waits on an event until signaled or until
 *      abstime passes.
 *      If abstime has passed when this routine is called then
 *      it returns a result to indicate this.
 *
 *      If 'abstime' is a NULL pointer then this function will
 *      block until it can successfully decrease the value or
 *      until interrupted by a signal.
 *
 *      This routine is not a cancelation point.
 *
 * RESULTS
 *              0               successfully signaled,
 *              ETIMEDOUT       abstime passed
 *              EINVAL          'event' is not a valid event,
 *
 * ------------------------------------------------------
 */
{

  unsigned int milliseconds;
  pte_osResult status;
  int retval;

  if (abstime == NULL)
    {
      status = pte_osSemaphorePend(event, NULL);
    }
  else
    {
      /*
       * Calculate timeout as milliseconds from current system time.
       */
      milliseconds = pte_relmillisecs (abstime);

      status = pte_osSemaphorePend(event, &milliseconds);
    }


  if (status == PTE_OS_TIMEOUT)
    {
      retval = ETIMEDOUT;
    }
  else
    {
      retval = 0;
    }

  return retval;

}				/* pte_timed_semwait */


int
pthread_mutex_timedlock (pthread_mutex_t * mutex,
                         const struct timespec *abstime)
{
  int result;
  pthread_mutex_t mx;

  /*
   * Let the system deal with invalid pointers.
   */

  /*
   * We do a quick check to see if we need to do more work
   * to initialise a static mutex. We check
   * again inside the guarded section of pte_mutex_check_need_init()
   * to avoid race conditions.
   */
  if (*mutex >= PTHREAD_ERRORCHECK_MUTEX_INITIALIZER)
    {
      if ((result = pte_mutex_check_need_init (mutex)) != 0)
        {
          return (result);
        }
    }

  mx = *mutex;

  if (mx->kind == PTHREAD_MUTEX_NORMAL)
    {
      if (PTE_ATOMIC_EXCHANGE(&mx->lock_idx,1) != 0)
        {
          while (PTE_ATOMIC_EXCHANGE(&mx->lock_idx,-1) != 0)
            {
              if (0 != (result = pte_timed_eventwait (mx->handle, abstime)))
                {
                  return result;
                }
            }
        }
    }
  else
    {
      pthread_t self = pthread_self();

      if (PTE_ATOMIC_COMPARE_EXCHANGE(&mx->lock_idx,1,0) == 0)
        {
          mx->recursive_count = 1;
          mx->ownerThread = self;
        }
      else
        {
          if (pthread_equal (mx->ownerThread, self))
            {
              if (mx->kind == PTHREAD_MUTEX_RECURSIVE)
                {
                  mx->recursive_count++;
                }
              else
                {
                  return EDEADLK;
                }
            }
          else
            {
              while (PTE_ATOMIC_EXCHANGE(&mx->lock_idx,-1) != 0)
                {
                  if (0 != (result = pte_timed_eventwait (mx->handle, abstime)))
                    {
                      return result;
                    }
                }

              mx->recursive_count = 1;
              mx->ownerThread = self;
            }
        }
    }

  return 0;
}
