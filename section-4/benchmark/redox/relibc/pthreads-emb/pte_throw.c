/*
 * pte_throw.c
 *
 * Description:
 * This translation unit implements routines which are private to
 * the implementation and may be used throughout it.
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
#include <stdlib.h>

#include "pthread.h"
#include "implement.h"

/*
 * pte_throw
 *
 * All canceled and explicitly exited POSIX threads go through
 * here. This routine knows how to exit both POSIX initiated threads and
 * 'implicit' POSIX threads for each of the possible language modes (C,
 * C++).
 */
void
pte_throw (unsigned int exception)
{
  /*
   * Don't use pthread_self() to avoid creating an implicit POSIX thread handle
   * unnecessarily.
   */
  pte_thread_t * sp = (pte_thread_t *) pthread_getspecific (pte_selfThreadKey);


  if (exception != PTE_EPS_CANCEL && exception != PTE_EPS_EXIT)
    {
      /* Should never enter here */
      exit (1);
    }

  if (NULL == sp || sp->implicit)
    {
      /*
       * We're inside a non-POSIX initialised OS thread
       * so there is no point to jump or throw back to. Just do an
       * explicit thread exit here after cleaning up POSIX
       * residue (i.e. cleanup handlers, POSIX thread handle etc).
       */
      unsigned exitCode = 0;

      switch (exception)
        {
        case PTE_EPS_CANCEL:
          exitCode = (unsigned) PTHREAD_CANCELED;
          break;
        case PTE_EPS_EXIT:
          exitCode = (unsigned) sp->exitStatus;;
          break;
        }

      pte_thread_detach_and_exit_np ();

//      pte_osThreadExit((void*)exitCode);

    }

#ifdef PTE_CLEANUP_C

  pte_pop_cleanup_all (1);
  longjmp (sp->start_mark, exception);

#else /* PTE_CLEANUP_C */

#ifdef PTE_CLEANUP_CXX

  switch (exception)
    {
    case PTE_EPS_CANCEL:
      throw pte_exception_cancel ();
      break;
    case PTE_EPS_EXIT:
      throw pte_exception_exit ();
      break;
    }

#else

#error ERROR [__FILE__, line __LINE__]: Cleanup type undefined.

#endif /* PTE_CLEANUP_CXX */

#endif /* PTE_CLEANUP_C */

  /* Never reached */
}


void
pte_pop_cleanup_all (int execute)
{
  while (NULL != pte_pop_cleanup (execute))
    {
    }
}


unsigned int
pte_get_exception_services_code (void)
{
  return (unsigned int) NULL;
}
