/*
 * global.c
 *
 * Description:
 * This translation unit instantiates data associated with the implementation
 * as a whole.
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


int pte_processInitialized = PTE_FALSE;
pte_thread_t * pte_threadReuseTop = PTE_THREAD_REUSE_EMPTY;
pte_thread_t * pte_threadReuseBottom = PTE_THREAD_REUSE_EMPTY;
pthread_key_t pte_selfThreadKey = NULL;
pthread_key_t pte_cleanupKey = NULL;
pthread_cond_t pte_cond_list_head = NULL;
pthread_cond_t pte_cond_list_tail = NULL;

int pte_concurrency = 0;

/* What features have been auto-detaected */
int pte_features = 0;

unsigned char pte_smp_system = PTE_TRUE;  /* Safer if assumed true initially. */

/*
 * Global lock for managing pthread_t struct reuse.
 */
pte_osMutexHandle pte_thread_reuse_lock;

/*
 * Global lock for testing internal state of statically declared mutexes.
 */
pte_osMutexHandle pte_mutex_test_init_lock;

/*
 * Global lock for testing internal state of PTHREAD_COND_INITIALIZER
 * created condition variables.
 */
pte_osMutexHandle pte_cond_test_init_lock;

/*
 * Global lock for testing internal state of PTHREAD_RWLOCK_INITIALIZER
 * created read/write locks.
 */
pte_osMutexHandle pte_rwlock_test_init_lock;

/*
 * Global lock for testing internal state of PTHREAD_SPINLOCK_INITIALIZER
 * created spin locks.
 */
pte_osMutexHandle pte_spinlock_test_init_lock;

/*
 * Global lock for condition variable linked list. The list exists
 * to wake up CVs when a WM_TIMECHANGE message arrives. See
 * w32_TimeChangeHandler.c.
 */
pte_osMutexHandle pte_cond_list_lock;


