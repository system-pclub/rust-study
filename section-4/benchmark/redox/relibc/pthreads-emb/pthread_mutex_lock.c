/*
 * pthread_mutex_lock.c
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

#include <stdio.h>
#include <stdlib.h>

#include "pthread.h"
#include "implement.h"


int
pthread_mutex_lock (pthread_mutex_t * mutex)
{
  int result = 0;
  pthread_mutex_t mx;

  /*
   * Let the system deal with invalid pointers.
   */
  if (*mutex == NULL)
    {
      return EINVAL;
    }

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
      if (PTE_ATOMIC_EXCHANGE(
            &mx->lock_idx,
            1) != 0)
        {
          while (PTE_ATOMIC_EXCHANGE(&mx->lock_idx,-1) != 0)
            {
              if (pte_osSemaphorePend(mx->handle,NULL) != PTE_OS_OK)
                {
                  result = EINVAL;
                  break;
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
                  result = EDEADLK;
                }
            }
          else
            {
              while (PTE_ATOMIC_EXCHANGE(&mx->lock_idx,-1) != 0)
                {
                  if (pte_osSemaphorePend(mx->handle,NULL) != PTE_OS_OK)
                    {
                      result = EINVAL;
                      break;
                    }
                }

              if (0 == result)
                {
                  mx->recursive_count = 1;
                  mx->ownerThread = self;
                }
            }
        }

    }

  return (result);
}
