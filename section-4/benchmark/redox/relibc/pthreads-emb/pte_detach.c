/*
 * pthread_win32_attach_detach_np.c
 *
 * Description:
 * This translation unit implements non-portable thread functions.
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

static int
pte_thread_detach_common (unsigned char threadShouldExit)
{
  if (pte_processInitialized)
    {
      /*
       * Don't use pthread_self() - to avoid creating an implicit POSIX thread handle
       * unnecessarily.
       */
      pte_thread_t * sp = (pte_thread_t *) pthread_getspecific (pte_selfThreadKey);

      if (sp != NULL) // otherwise OS thread with no implicit POSIX handle.
        {

          pte_callUserDestroyRoutines (sp->ptHandle);

          (void) pthread_mutex_lock (&sp->cancelLock);
          sp->state = PThreadStateLast;

          /*
           * If the thread is joinable at this point then it MUST be joined
           * or detached explicitly by the application.
           */
          (void) pthread_mutex_unlock (&sp->cancelLock);

          if (sp->detachState == PTHREAD_CREATE_DETACHED)
            {
              if (threadShouldExit)
                {
                  pte_threadExitAndDestroy (sp->ptHandle);
                }
              else
                {
                  pte_threadDestroy (sp->ptHandle);
                }

              // pte_osTlsSetValue (pte_selfThreadKey->key, NULL);
            }
          else
            {
              if (threadShouldExit)
                {
                  pte_osThreadExit();
                }
            }
        }
    }

  return 1;
}

int pte_thread_detach_and_exit_np()
{
  return pte_thread_detach_common(1);
}

int pte_thread_detach_np()
{
  return pte_thread_detach_common(0);
}

