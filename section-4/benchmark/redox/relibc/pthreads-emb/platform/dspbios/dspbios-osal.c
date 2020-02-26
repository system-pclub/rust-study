/*
 * psp_osal.h
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

#include <std.h>
#include <clk.h>
#include <lck.h>
#include <mbx.h>

#include <pthread.h>

#include "tls-helper.h"
#include "pte_osal.h"

#define POLLING_DELAY_IN_ticks 10

#define DSPBIOS_MAX_TLS 32

#define DEFAULT_STACK_SIZE_BYTES 4096

/*
 * Data stored on a per-thread basis - allocated in pte_osThreadCreate
 * and freed in pte_osThreadDelete.
 */
typedef struct dspbiosThreadData
  {
    /* Semaphore used to wait for threads to end.  Posted to in pte_osThreadExit() */
    SEM_Handle joinSem;

    /* Semaphore used for cancellation.  Posted to by pte_osThreadCancel, polled in pte_osSemaphoreCancellablePend */
    SEM_Handle cancelSem;

    /* Initial priority of the thread. */
    int priority;

  } dspbiosThreadData;


/* Task and mailbox used for cleaning up detached threads */
static TSK_Handle gcTaskHandle;
static MBX_Handle gcMailbox;

/* TLS key used to access dspbiosThreadData struct for reach thread. */
static unsigned int threadDataKey;


/*
 *
 * Helper functions
 *
 */

/* Returns a pointer to the per thread control info */
static dspbiosThreadData * getThreadData(TSK_Handle threadHandle)
{

  dspbiosThreadData *pThreadData;
  void * pTls;

  pTls = (void *) TSK_getenv(threadHandle);

  pThreadData = (dspbiosThreadData *) pteTlsGetValue(pTls, threadDataKey);

  return pThreadData;
}

/* Converts milliseconds to system ticks (for TSK_sleep, SEM_pend, etc) */
static int msecsToSysTicks(int msecs)
{
  int ticks = CLK_countspms() / CLK_getprd() * msecs;

  ticks = ticks / 100; // sim only

  return ticks;
}

/* Garbage collector thread to free resources from detached threads */
void dspbiosGarbageCollectorMain()
{

  gcMailbox = MBX_create(sizeof(void*), 10, NULL);

  while (1)
    {
      Bool status;
      void * pTask;

      /* Wait for dying threads to post their handles to our mailbox */
      status = MBX_pend(gcMailbox, &pTask, SYS_FOREVER);

      if (status)
        {
          TSK_delete((TSK_Handle) pTask);
        }
    }

  /* Never returns */
}

/****************************************************************************
 *
 * Initialization
 *
 ***************************************************************************/

/*
 * Initializes the OSAL.
 *
 *   1. Initialize TLS support.
 *   2. Allocate control data TLS key.
 *   3. Start garbage collector thread.
 */
pte_osResult pte_osInit(void)
{
  pte_osResult result;
  TSK_Attrs attrs;

  /* Allocate and initialize TLS support */
  result = pteTlsGlobalInit(DSPBIOS_MAX_TLS);
  
  if (result == PTE_OS_OK)
    {
      /* Allocate a key that we use to store control information (e.g. cancellation semaphore) per thread */
      result = pteTlsAlloc(&threadDataKey);

      if (result == PTE_OS_OK)
	{
	  /* Create a low priority task to free resources for detached threads */
	  attrs = TSK_ATTRS;
	  attrs.priority = 1; /* just above idle task */
	  attrs.name = "pthread-gc";

	  gcTaskHandle = TSK_create((Fxn) dspbiosGarbageCollectorMain, &attrs);

	  /* Give the garbage collector task a chance to run and create the mailbox */
	  TSK_sleep(1);

	  if (gcTaskHandle == NULL)
	    {
	      result = PTE_OS_NO_RESOURCES;
	    }
	}
    }

  return result;
}

/****************************************************************************
 *
 * Threads
 *
 ***************************************************************************/


/* Entry point for new threads */
void dspbiosStubThreadEntry (void *argv, pte_osThreadEntryPoint entryPoint)
{
  (*(entryPoint))(argv);
  return;
}

/*
 * Creates a new thread, allocates resources, etc.
 *
 * The thread is created in a suspended state; execution will actually start
 * when pte_osThreadStart() is called.  Setting the priority to a -1 will start the
 * thread in a suspended state.  pte_osThreadStart then sets the real priority which
 * will start the thread executing.
 *
 * In order for dynamic tasks to work, you must set up a heap for DSP/BIOS to allocate their
 * stacks from.  This should be done in the projects tcf or cdb file.
 *
 */
pte_osResult pte_osThreadCreate(pte_osThreadEntryPoint entryPoint,
                                int stackSize,
                                int initialPriority,
                                void *argv,
                                pte_osThreadHandle* ppte_osThreadHandle)
{
  TSK_Handle handle;
  TSK_Attrs attrs;
  void *pTls;
  pte_osResult result;
  dspbiosThreadData *pThreadData;

  /* Make sure that the stack we're going to allocate is big enough */
  if (stackSize < DEFAULT_STACK_SIZE_BYTES)
    {
      stackSize = DEFAULT_STACK_SIZE_BYTES;
    }

  /* Allocate TLS structure for this thread. */
  pTls = pteTlsThreadInit();
  if (pTls == NULL)
    {
      result = PTE_OS_NO_RESOURCES;
      goto FAIL0;
    }


  /* Allocate some memory for our per-thread control data.  We use this for:
   *   1. join semaphore (used to wait for thread termination)
   *   2. cancellation semaphore (used to signal a thread to cancel)
   *   3. initial priority (used in stub entry point)
   */
  pThreadData = (dspbiosThreadData*) malloc(sizeof(dspbiosThreadData));

  if (pThreadData == NULL)
    {
      pteTlsThreadDestroy(pTls);

      result = PTE_OS_NO_RESOURCES;
      goto FAIL0;
    }

  /* Save a pointer to our per-thread control data as a TLS value */
  pteTlsSetValue(pTls, threadDataKey, pThreadData);

  /* Create semaphores and save handles */
  pThreadData->joinSem = SEM_create(0, NULL);
  pThreadData->cancelSem = SEM_create(0, NULL);

  /* Save the initial priority - we need it when we
   * actually start the thread */
  pThreadData->priority = initialPriority;

  /*
   * Fill out parameters for TSK_create:
   */
  attrs = TSK_ATTRS;

  /* Use  value specified by user */
  attrs.stacksize = stackSize;

  attrs.priority  = -1;

  /* Save our TLS structure as the task's environment. */
  attrs.environ   = pTls;

  handle = TSK_create((Fxn) dspbiosStubThreadEntry, &attrs, argv, entryPoint);

  if (handle != NULL)
    {
      /* Everything worked, return handle to caller */
      *ppte_osThreadHandle = handle;
      result = PTE_OS_OK;
    }
  else
    {
      /* Something went wrong - assume that it was due to lack of resources. */
      free(pThreadData);
      pteTlsThreadDestroy(pTls);

      result = PTE_OS_NO_RESOURCES;
    }

FAIL0:
  return result;
}

/* Start executing a thread.
 *
 * Get the priority that the user specified when they called
 * pte_osThreadCreate and set the priority of the thread.  This
 * will start the thread executing.
 */
pte_osResult pte_osThreadStart(pte_osThreadHandle osThreadHandle)
{

  dspbiosThreadData *pThreadData;

  pThreadData = getThreadData(osThreadHandle);

  TSK_setpri(osThreadHandle, pThreadData->priority);

  return PTE_OS_OK;

}


/*
 * Exit from a thread.
 *
 * Post to the join semaphore in case a pthread_join is
 * waiting for us to exit.
 */
void pte_osThreadExit()
{
  TSK_Handle thisTask;
  dspbiosThreadData *pThreadData;

  thisTask = TSK_self();

  pThreadData = getThreadData(thisTask);

  if (pThreadData != NULL)
    {
      SEM_post(pThreadData->joinSem);
    }

  TSK_exit();

}

pte_osResult pte_osThreadExitAndDelete(pte_osThreadHandle handle)
{
  dspbiosThreadData *pThreadData;
  void * pTls;

  pThreadData = getThreadData(handle);

  pTls = (void *) TSK_getenv(handle);

  /* Free the per thread data (join & cancel semaphores, etc) */
  SEM_delete(pThreadData->joinSem);
  SEM_delete(pThreadData->cancelSem);

  free(pThreadData);

  /* Free the TLS data structure */
  pteTlsThreadDestroy(pTls);

  /* Send thread handle to garbage collector task so it can free
   * resources from a different context */
  MBX_post(gcMailbox, &handle, SYS_FOREVER);

  TSK_exit();
  
  return PTE_OS_OK;
}

/* Clean up a thread.
 *
 * If this is called from the thread itself, instead of actually freeing up
 * resources, post the thread handle to a mailbox which will be picked
 * up by the garbage collector thread.  Resources will be freed from there.
 * This is necessary because DSP/BIOS does not free resources when a
 * thread exits.
 */
pte_osResult pte_osThreadDelete(pte_osThreadHandle handle)
{

  dspbiosThreadData *pThreadData;
  void * pTls;

  pThreadData = getThreadData(handle);

  pTls = (void *) TSK_getenv(handle);

  /* Free the per thread data (join & cancel semaphores, etc) */
  SEM_delete(pThreadData->joinSem);
  SEM_delete(pThreadData->cancelSem);

  free(pThreadData);

  /* Free the TLS data structure */
  pteTlsThreadDestroy(pTls);

  TSK_delete(handle);

  return PTE_OS_OK;
}

/* Wait for a thread to exit.
 *
 * Since DSP/BIOS doesn't have a explicit system call for this, we
 * emulate it using a semaphore that is posted to when the thread
 * exits.
 */
pte_osResult pte_osThreadWaitForEnd(pte_osThreadHandle threadHandle)
{
  TSK_Stat taskStats;
  pte_osResult result;

  /* Prevent context switches to prevent the thread from
   * changing states */
  TSK_disable();

  /* First, check if the thread has already terminated. */
  TSK_stat(threadHandle, &taskStats);

  if (taskStats.mode != TSK_TERMINATED)
    {
      dspbiosThreadData *pThreadData;
      dspbiosThreadData *pSelfThreadData;

      pThreadData = getThreadData(threadHandle);
      pSelfThreadData = getThreadData(TSK_self());

      TSK_enable();

      /* This needs to be cancellable, so poll instead of block,
       * similar to what we do for pte_OsSemaphoreCancellablePend. */
      while (1)
	{
	  if (SEM_count(pThreadData->joinSem) > 0)
	    {
	      /* The thread has exited. */
	      result = PTE_OS_OK;
	      break;
	    }
	  else if ((pSelfThreadData != NULL) && SEM_count(pSelfThreadData->cancelSem) > 0)
	    {
	      /* The thread was cancelled */
	      result = PTE_OS_INTERRUPTED;
	      break;
	    }
          else
            {
              /* Nothing found and not timed out yet; let's yield so we're not
               * in busy loop. */
              TSK_sleep(POLLING_DELAY_IN_ticks);
            }
        }

    }
  else
    {
      /* Thread is already terminated, just return OK */
      TSK_enable();
      result = PTE_OS_OK;
    }

  return result;
}

/* Cancels the specified thread.  This will 1) make pte_osSemaphoreCancellablePend return if it is currently
 * blocked and will make pte_osThreadCheckCancel return TRUE.
 *
 * To accomplish this, we post to the cancellation semaphore for the specified thread.
 */
pte_osResult pte_osThreadCancel(pte_osThreadHandle threadHandle)
{
  dspbiosThreadData *pThreadData;

  pThreadData = getThreadData(threadHandle);

  if (pThreadData != NULL)
    {
      SEM_post(pThreadData->cancelSem);
    }

  return PTE_OS_OK;
}


/*
 * Checks to see if pte_osThreadCancel has been called for the specified thread.  Just check the
 * value of the cancellation semaphore associated with the thread.
 */
pte_osResult pte_osThreadCheckCancel(pte_osThreadHandle threadHandle)
{

  dspbiosThreadData *pThreadData;

  pThreadData = getThreadData(threadHandle);

  if (pThreadData != NULL)
    {
      if (SEM_count(pThreadData->cancelSem) > 0)
        {
          return PTE_OS_INTERRUPTED;
        }
      else
        {
          return PTE_OS_OK;
        }
    }
  else
    {
      /* We're being called from a pure OS thread which can't be cancelled. */
      return PTE_OS_OK;
    }


}

void pte_osThreadSleep(unsigned int msecs)
{
  int ticks = msecsToSysTicks(msecs);

  TSK_sleep(ticks);
}

pte_osThreadHandle pte_osThreadGetHandle(void)
{
  return TSK_self();
}

int pte_osThreadGetPriority(pte_osThreadHandle threadHandle)
{
  return TSK_getpri(threadHandle);
}

pte_osResult pte_osThreadSetPriority(pte_osThreadHandle threadHandle, int newPriority)
{
  TSK_setpri(threadHandle, newPriority);

  return PTE_OS_OK;
}

int pte_osThreadGetMinPriority()
{
  return TSK_MINPRI;
}

int pte_osThreadGetMaxPriority()
{
  return TSK_MAXPRI;
}

int pte_osThreadGetDefaultPriority()
{
  /* Pick something in the middle */
  return ((TSK_MINPRI + TSK_MAXPRI) / 2);
}

/****************************************************************************
 *
 * Mutexes
 *
 ***************************************************************************/

pte_osResult pte_osMutexCreate(pte_osMutexHandle *pHandle)
{

  *pHandle = LCK_create(NULL);

  if (*pHandle == NULL)
    {
      return PTE_OS_NO_RESOURCES;
    }
  else
    {
      return PTE_OS_OK;
    }
}

pte_osResult pte_osMutexDelete(pte_osMutexHandle handle)
{
  LCK_delete(handle);

  return PTE_OS_OK;
}

pte_osResult pte_osMutexLock(pte_osMutexHandle handle)
{
  LCK_pend(handle, SYS_FOREVER);

  return PTE_OS_OK;
}

pte_osResult pte_osMutexUnlock(pte_osMutexHandle handle)
{

  LCK_post(handle);

  return PTE_OS_OK;
}

/****************************************************************************
 *
 * Semaphores
 *
 ***************************************************************************/

pte_osResult pte_osSemaphoreCreate(int initialValue, pte_osSemaphoreHandle *pHandle)
{

  *pHandle = SEM_create(initialValue, NULL);

  if (*pHandle == NULL)
    {
      return PTE_OS_NO_RESOURCES;
    }
  else
    {
      return PTE_OS_OK;
    }

}

pte_osResult pte_osSemaphoreDelete(pte_osSemaphoreHandle handle)
{
  SEM_delete(handle);

  return PTE_OS_OK;
}


pte_osResult pte_osSemaphorePost(pte_osSemaphoreHandle handle, int count)
{
  int i;

  for (i=0;i<count;i++)
    {
      SEM_post(handle);
    }

  return PTE_OS_OK;
}

pte_osResult pte_osSemaphorePend(pte_osSemaphoreHandle handle, unsigned int *pTimeoutMsecs)
{
  Bool status;
  unsigned int timeoutTicks;

  if (pTimeoutMsecs == NULL)
    {
      timeoutTicks = SYS_FOREVER;
    }
  else
    {
      timeoutTicks = msecsToSysTicks(*pTimeoutMsecs);
    }


  status = SEM_pend(handle, timeoutTicks);

  if (status)
    {
      return PTE_OS_OK;
    }
  else
    {
      return PTE_OS_TIMEOUT;
    }

}



/*
 * Pend on a semaphore- and allow the pend to be cancelled.
 *
 * DSP/BIOS provides no functionality to asynchronously interrupt a blocked call.  We simulte
 * this by polling on the main semaphore and the cancellation semaphore and sleeping in a loop.
 */
pte_osResult pte_osSemaphoreCancellablePend(pte_osSemaphoreHandle semHandle, unsigned int *pTimeout)
{


  clock_t start_time;
  clock_t current_time;
  pte_osResult result =  PTE_OS_OK;
  int timeout;

  dspbiosThreadData *pThreadData;

  pThreadData = getThreadData(TSK_self());


  start_time = CLK_getltime();

  if (pTimeout == NULL)
    {
      timeout = -1;
    }
  else
    {
      timeout = msecsToSysTicks(*pTimeout);
    }

  while (1)
    {
      Bool semTimeout;
      int status;

      /* Poll semaphore */
      semTimeout = 0;

      current_time = CLK_getltime();

      status = SEM_pend(semHandle, semTimeout);

      if (status == TRUE)
        {
          /* The user semaphore was posted to */
          result = PTE_OS_OK;
          break;
        }
      else if ((timeout != -1) && ((current_time - start_time) > timeout))
        {
          /* The timeout expired */
          result = PTE_OS_TIMEOUT;
          break;
        }
      else
        {
          if ((pThreadData != NULL) && SEM_count(pThreadData->cancelSem) > 0)
            {
              /* The thread was cancelled */
              result = PTE_OS_INTERRUPTED;
              break;
            }
          else
            {
              /* Nothing found and not timed out yet; let's yield so we're not
               * in busy loop. */
              TSK_sleep(POLLING_DELAY_IN_ticks);
            }
        }

    }

  return result;

}


/****************************************************************************
 *
 * Atomic Operations
 *
 ***************************************************************************/
int pte_osAtomicExchange(int *ptarg, int val)
{

  long origVal;
  Uns oldCSR;

  oldCSR = HWI_disable();

  origVal = *ptarg;

  *ptarg = val;

  HWI_restore(oldCSR);

  return origVal;

}

int pte_osAtomicCompareExchange(int *pdest, int exchange, int comp)
{

  int origVal;
  Uns oldCSR;

  oldCSR = HWI_disable();


  origVal = *pdest;

  if (*pdest == comp)
    {
      *pdest = exchange;
    }

  HWI_restore(oldCSR);


  return origVal;

}

int pte_osAtomicExchangeAddInt(int volatile* pAddend, int value)
{

  int origVal;
  Uns oldCSR;

  oldCSR = HWI_disable();


  origVal = *pAddend;

  *pAddend += value;

  HWI_restore(oldCSR);


  return origVal;

}

int pte_osAtomicExchangeAdd(int volatile* pAddend, int value)
{

  int origVal;
  Uns oldCSR;

  oldCSR = HWI_disable();


  origVal = *pAddend;

  *pAddend += value;

  HWI_restore(oldCSR);


  return origVal;
}

int pte_osAtomicDecrement(int *pdest)
{
  int val;
  Uns oldCSR;

  oldCSR = HWI_disable();


  (*pdest)--;
  val = *pdest;

  HWI_restore(oldCSR);

  return val;

}

int pte_osAtomicIncrement(int *pdest)
{
  int val;
  Uns oldCSR;

  oldCSR = HWI_disable();

  (*pdest)++;

  val = *pdest;

  HWI_restore(oldCSR);


  return val;
}

/****************************************************************************
 *
 * Thread Local Storage
 *
 ***************************************************************************/

pte_osResult pte_osTlsSetValue(unsigned int index, void * value)
{
  void *pTls;

  pTls = (void *) TSK_getenv(TSK_self());

  if (pTls == NULL)
    {
      // Apparently no TLS structure has been allocated for this thread
      // Probably because this is a native OS thread
      pTls = pteTlsThreadInit();


      TSK_setenv(TSK_self(), pTls);
    }

  return pteTlsSetValue(pTls, index, value);
}



void * pte_osTlsGetValue(unsigned int index)
{
  void *pTls;

  pTls = (void *) TSK_getenv(TSK_self());

  return (void *) pteTlsGetValue(pTls, index);

}


// Note that key value must be > 0
pte_osResult pte_osTlsAlloc(unsigned int *pKey)
{
  return pteTlsAlloc(pKey);
}


pte_osResult pte_osTlsFree(unsigned int index)
{
  return pteTlsFree(index);

}


int ftime(struct timeb *tp)
{
  int ltime = (CLK_getltime() * CLK_cpuCyclesPerLtime()) / CLK_cpuCyclesPerLtime();
  int secs = ltime / 1000;
  int msecs = ltime % 1000;

  tp->dstflag = 0;
  tp->timezone = 0;
  tp->time = secs;
  tp->millitm = msecs;

  return 0;
}


