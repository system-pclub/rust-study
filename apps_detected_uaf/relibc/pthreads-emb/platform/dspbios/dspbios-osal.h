/*
 * dspbios-osal.h
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

#include <std.h>
#include <tsk.h>
#include <sem.h>
#include <lck.h>
#include <errno.h>

typedef TSK_Handle pte_osThreadHandle;

typedef SEM_Handle pte_osSemaphoreHandle;

typedef LCK_Handle pte_osMutexHandle;

#define OS_DEFAULT_PRIO 11

#define OS_MAX_SIMUL_THREADS 10




#ifndef EPERM
#define EPERM           1
#endif // EPERM

#ifndef ESRCH
#define ESRCH           3
#endif // ESRCH

#ifndef EINTR
#define EINTR           4
#endif // EINTR

#ifndef EIO
#define EIO             5
#endif // EIO

#ifndef EAGAIN
#define EAGAIN          11
#endif // EAGAIN

#ifndef ENOMEM
#define ENOMEM          12
#endif // ENOMEM

#ifndef EBUSY
#define EBUSY           16
#endif // EBUSY

#ifndef EINVAL
#define EINVAL          22
#endif // EINVAL

#ifndef ENOSPC
#define ENOSPC          28
#endif // ENOSPC

#ifndef EDEADLK
#define EDEADLK         35
#endif /* EDEADLK */

#ifndef ENOSYS
#define ENOSYS          38
#endif /* ENOSYS */

#ifndef ENOTSUP
#define ENOTSUP         95
#endif /* ENOTSUP */

#ifndef ETIMEDOUT
#define ETIMEDOUT       116
#endif // ETIMEDOUT




