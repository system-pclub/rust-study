/*
 * pte_threadStart.c
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

#include <stdio.h>
#include <stdlib.h>

#include "pthread.h"
#include "implement.h"

#if defined(PTE_CLEANUP_CXX)

# if defined(__GNUC__) && __GNUC__ < 3
#   include <new>
# else
#   include <new>
using
std::terminate_handler;
using
std::terminate;
using
std::set_terminate;
# endif

typedef terminate_handler terminate_function;

static terminate_function pte_oldTerminate;

void
pte_terminate ()
{
  set_terminate (pte_oldTerminate);
  (void) pte_thread_detach_np();
//  terminate ();
}

#endif

int pte_threadStart (void *vthreadParms)
{
  ThreadParms * threadParms = (ThreadParms *) vthreadParms;
  pthread_t self;
  pte_thread_t * sp;
  void *(*start) (void *);
  void * arg;

#ifdef PTE_CLEANUP_C
#include <setjmp.h>

  int setjmp_rc;
#endif

  void * status = (void *) 0;

  self = threadParms->tid;
  sp = (pte_thread_t *) self;
  start = threadParms->start;
  arg = threadParms->arg;
//  free (threadParms);

  pthread_setspecific (pte_selfThreadKey, sp);

  sp->state = PThreadStateRunning;

#ifdef PTE_CLEANUP_C


  setjmp_rc = setjmp (sp->start_mark);


  if (0 == setjmp_rc)
    {

      /*
       * Run the caller's routine;
       */
      sp->exitStatus = status = (*start) (arg);
    }
  else
    {
      switch (setjmp_rc)
        {
        case PTE_EPS_CANCEL:
          status = sp->exitStatus = PTHREAD_CANCELED;
          break;
        case PTE_EPS_EXIT:
          status = sp->exitStatus;
          break;
        default:
          status = sp->exitStatus = PTHREAD_CANCELED;
          break;
        }
    }

#else /* PTE_CLEANUP_C */

#ifdef PTE_CLEANUP_CXX

  pte_oldTerminate = set_terminate (&pte_terminate);

  try
    {
      /*
      * Run the caller's routine in a nested try block so that we
      * can run the user's terminate function, which may call
      * pthread_exit() or be canceled.
      */
      try
        {
          status = sp->exitStatus = (*start) (arg);
        }
      catch (pte_exception &)
        {
          /*
          * Pass these through to the outer block.
          */
          throw;
        }
      catch (...)
        {
          /*
          * We want to run the user's terminate function if supplied.
          * That function may call pthread_exit() or be canceled, which will
          * be handled by the outer try block.
          *
          * pte_terminate() will be called if there is no user
          * supplied function.
          */

          terminate_function
          term_func = set_terminate (0);
          set_terminate (term_func);

          if (term_func != 0)
            {
              term_func ();
            }

          throw;
        }
    }
  catch (pte_exception_cancel &)
    {
      /*
      * Thread was canceled.
      */
      status = sp->exitStatus = PTHREAD_CANCELED;
    }
  catch (pte_exception_exit &)
    {
      /*
      * Thread was exited via pthread_exit().
      */
      status = sp->exitStatus;
    }
  catch (...)
    {
      /*
      * A system unexpected exception has occurred running the user's
      * terminate routine. We get control back within this block - cleanup
      * and release the exception out of thread scope.
      */
      status = sp->exitStatus = PTHREAD_CANCELED;
      (void) pthread_mutex_lock (&sp->cancelLock);
      sp->state = PThreadStateException;
      (void) pthread_mutex_unlock (&sp->cancelLock);
      (void) pte_thread_detach_np();
      (void) set_terminate (pte_oldTerminate);
      throw;

      /*
      * Never reached.
      */
    }

  (void) set_terminate (pte_oldTerminate);

#else

#error ERROR [__FILE__, line __LINE__]: Cleanup type undefined.

#endif /* PTE_CLEANUP_CXX */
#endif /* PTE_CLEANUP_C */

  /*
   * We need to cleanup the pthread now if we have
   * been statically linked, in which case the cleanup
   * in dllMain won't get done. Joinable threads will
   * only be partially cleaned up and must be fully cleaned
   * up by pthread_join() or pthread_detach().
   *
   * Note: if this library has been statically linked,
   * implicitly created pthreads (those created
   * for OS threads which have called pthreads routines)
   * must be cleaned up explicitly by the application
   * (by calling pte_thread_detach_np()).
   */
  (void) pte_thread_detach_and_exit_np ();

  //pte_osThreadExit(status);

  /*
   * Never reached.
   */

  return (unsigned) status;

}				/* pte_threadStart */
