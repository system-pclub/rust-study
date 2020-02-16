/*
 * pthread_once.c
 *
 * Description:
 * This translation unit implements miscellaneous thread functions.
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

#include "pte_osal.h"
#include "pthread.h"
#include "implement.h"

#define PTE_ONCE_STARTED 1
#define PTE_ONCE_INIT 0
#define PTE_ONCE_DONE 2

static void
pte_once_init_routine_cleanup(void * arg)
{
  pthread_once_t * once_control = (pthread_once_t *) arg;

  (void) PTE_ATOMIC_EXCHANGE(&once_control->state,PTE_ONCE_INIT);

  if (PTE_ATOMIC_EXCHANGE_ADD((int*)&once_control->semaphore, 0L)) /* MBR fence */
    {
      pte_osSemaphorePost((pte_osSemaphoreHandle) once_control->semaphore, 1);
    }
}

int
pthread_once (pthread_once_t * once_control, void (*init_routine) (void))
/*
 * ------------------------------------------------------
 * DOCPUBLIC
 *      If any thread in a process  with  a  once_control  parameter
 *      makes  a  call to pthread_once(), the first call will summon
 *      the init_routine(), but  subsequent  calls  will  not. The
 *      once_control  parameter  determines  whether  the associated
 *      initialization routine has been called.  The  init_routine()
 *      is complete upon return of pthread_once().
 *      This function guarantees that one and only one thread
 *      executes the initialization routine, init_routine when
 *      access is controlled by the pthread_once_t control
 *      key.
 *
 *      pthread_once() is not a cancelation point, but the init_routine
 *      can be. If it's cancelled then the effect on the once_control is
 *      as if pthread_once had never been entered.
 *
 *
 * PARAMETERS
 *      once_control
 *              pointer to an instance of pthread_once_t
 *
 *      init_routine
 *              pointer to an initialization routine
 *
 *
 * DESCRIPTION
 *      See above.
 *
 * RESULTS
 *              0               success,
 *              EINVAL          once_control or init_routine is NULL
 *
 * ------------------------------------------------------
 */
{
  int result;
  int state;
  pte_osSemaphoreHandle sema;

  if (once_control == NULL || init_routine == NULL)
    {
      result = EINVAL;
      goto FAIL0;
    }
  else
    {
      result = 0;
    }

  while ((state =
            PTE_ATOMIC_COMPARE_EXCHANGE(&once_control->state,
                                        PTE_ONCE_STARTED,
                                        PTE_ONCE_INIT))
         != PTE_ONCE_DONE)
    {
      if (PTE_ONCE_INIT == state)
        {


          pthread_cleanup_push(pte_once_init_routine_cleanup, (void *) once_control);
          (*init_routine)();
          pthread_cleanup_pop(0);

          (void) PTE_ATOMIC_EXCHANGE(&once_control->state,PTE_ONCE_DONE);

          /*
           * we didn't create the semaphore.
           * it is only there if there is someone waiting.
           */
          if (PTE_ATOMIC_EXCHANGE_ADD((int*)&once_control->semaphore, 0L)) /* MBR fence */
            {
              pte_osSemaphorePost((pte_osSemaphoreHandle) once_control->semaphore,once_control->numSemaphoreUsers);
            }
        }
      else
        {
          PTE_ATOMIC_INCREMENT(&once_control->numSemaphoreUsers);

          if (!PTE_ATOMIC_EXCHANGE_ADD((int*)&once_control->semaphore, 0L)) /* MBR fence */
            {
              pte_osSemaphoreCreate(0, (pte_osSemaphoreHandle*) &sema);

              if (PTE_ATOMIC_COMPARE_EXCHANGE((int *) &once_control->semaphore,
                                              (int) sema,
                                              0))
                {
                  pte_osSemaphoreDelete((pte_osSemaphoreHandle)sema);
                }
            }

          /*
           * Check 'state' again in case the initting thread has finished or
          * cancelled and left before seeing that there was a semaphore.
           */
          if (PTE_ATOMIC_EXCHANGE_ADD(&once_control->state, 0L) == PTE_ONCE_STARTED)
            {
              pte_osSemaphorePend((pte_osSemaphoreHandle) once_control->semaphore,NULL);
            }

          if (0 == PTE_ATOMIC_DECREMENT(&once_control->numSemaphoreUsers))
            {
              /* we were last */
              if ((sema =
                     (pte_osSemaphoreHandle) PTE_ATOMIC_EXCHANGE((int *) &once_control->semaphore,0)))
                {
                  pte_osSemaphoreDelete(sema);
                }
            }
        }
    }

  /*
   * ------------
   * Failure Code
   * ------------
   */
FAIL0:
  return (result);
}                               /* pthread_once */
