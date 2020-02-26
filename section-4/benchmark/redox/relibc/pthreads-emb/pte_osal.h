#ifndef _OS_SUPPORT_H_
#define _OS_SUPPORT_H_

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

#include <sys/types.h>

// XXX
typedef pid_t pte_osThreadHandle;
typedef unsigned long pte_osSemaphoreHandle;
typedef int32_t* pte_osMutexHandle;

#include <pte_generic_osal.h>

#ifdef __cplusplus
}
#endif // __cplusplus

#endif // _OS_SUPPORT_H
