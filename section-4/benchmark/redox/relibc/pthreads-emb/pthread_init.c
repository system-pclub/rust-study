
/*
 * pthread_init.c
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
#include "implement.h"

void pthread_init(void)
{

  if (pte_processInitialized)
    {
      /*
       * Ignore if already initialized. this is useful for
       * programs that uses a non-dll pthread
       * library. Such programs must call pte_processInitialize() explicitly,
       * since this initialization routine is automatically called only when
       * the dll is loaded.
       */
      return;
    }

  pte_processInitialized = PTE_TRUE;

  // Must happen before creating keys.
  pte_osInit();

  /*
   * Initialize Keys
   */
  if ((pthread_key_create (&pte_selfThreadKey, NULL) != 0) ||
      (pthread_key_create (&pte_cleanupKey, NULL) != 0))
    {
      pthread_terminate();
    }

  /*
   * Set up the global locks.
   */
  pte_osMutexCreate (&pte_thread_reuse_lock);
  pte_osMutexCreate (&pte_mutex_test_init_lock);
  pte_osMutexCreate (&pte_cond_list_lock);
  pte_osMutexCreate (&pte_cond_test_init_lock);
  pte_osMutexCreate (&pte_rwlock_test_init_lock);
  pte_osMutexCreate (&pte_spinlock_test_init_lock);


  return;
}
