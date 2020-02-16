/*
 * pte_threadReuse.c
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

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pthread.h"
#include "implement.h"


/*
 * How it works:
 * A pthread_t is a struct which is normally passed/returned by
 * value to/from pthreads routines.  Applications are therefore storing
 * a copy of the struct as it is at that time.
 *
 * The original pthread_t struct plus all copies of it contain the address of
 * the thread state struct pte_thread_t_ (p), plus a reuse counter (x). Each
 * pte_thread_t contains the original copy of it's pthread_t.
 * Once malloced, a pte_thread_t_ struct is not freed until the process exits.
 *
 * The thread reuse stack is a simple LILO stack managed through a singly
 * linked list element in the pte_thread_t.
 *
 * Each time a thread is destroyed, the pte_thread_t address is pushed onto the
 * reuse stack after it's ptHandle's reuse counter has been incremented.
 *
 * The following can now be said from this:
 * - two pthread_t's are identical if their pte_thread_t reference pointers
 * are equal and their reuse counters are equal. That is,
 *
 *   equal = (a.p == b.p && a.x == b.x)
 *
 * - a pthread_t copy refers to a destroyed thread if the reuse counter in
 * the copy is not equal to the reuse counter in the original.
 *
 *   threadDestroyed = (copy.x != ((pte_thread_t *)copy.p)->ptHandle.x)
 *
 */

/*
 * Pop a clean pthread_t struct off the reuse stack.
 */
pthread_t
pte_threadReusePop (void)
{
  pthread_t t = NULL;


  pte_osMutexLock (pte_thread_reuse_lock);

  if (PTE_THREAD_REUSE_EMPTY != pte_threadReuseTop)
    {
      pte_thread_t * tp;

      tp = pte_threadReuseTop;

      pte_threadReuseTop = tp->prevReuse;

      if (PTE_THREAD_REUSE_EMPTY == pte_threadReuseTop)
        {
          pte_threadReuseBottom = PTE_THREAD_REUSE_EMPTY;
        }

      tp->prevReuse = NULL;

      t = tp->ptHandle;
    }

  pte_osMutexUnlock(pte_thread_reuse_lock);

  return t;

}

/*
 * Push a clean pthread_t struct onto the reuse stack.
 * Must be re-initialised when reused.
 * All object elements (mutexes, events etc) must have been either
 * detroyed before this, or never initialised.
 */
void
pte_threadReusePush (pthread_t thread)
{
  pte_thread_t * tp = (pte_thread_t *) thread;
  pthread_t t;


  pte_osMutexLock (pte_thread_reuse_lock);

  t = tp->ptHandle;
  memset(tp, 0, sizeof(pte_thread_t));

  /* Must restore the original POSIX handle that we just wiped. */
  tp->ptHandle = t;

  tp->prevReuse = PTE_THREAD_REUSE_EMPTY;

  if (PTE_THREAD_REUSE_EMPTY != pte_threadReuseBottom)
    {
      pte_threadReuseBottom->prevReuse = tp;
    }
  else
    {
      pte_threadReuseTop = tp;
    }

  pte_threadReuseBottom = tp;

  pte_osMutexUnlock(pte_thread_reuse_lock);
}
