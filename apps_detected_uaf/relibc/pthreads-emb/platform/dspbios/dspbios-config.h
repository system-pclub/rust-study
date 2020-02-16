/* config.h  */

#ifndef PTE_CONFIG_H
#define PTE_CONFIG_H

#define PTE_STATIC_LIB

/*********************************************************************
 * Defaults: see target specific redefinitions below.
 *********************************************************************/

/* We're building the pthreads-win32 library */
#define PTE_BUILD

/* Define if you don't have Win32 errno. (eg. WinCE) */
#undef NEED_ERRNO
#define NEED_ERRNO

/* Do we know about type mode_t? */
#undef HAVE_MODE_T

/* Define if you have the timespec struct */
#undef HAVE_STRUCT_TIMESPEC
//#define HAVE_STRUCT_TIMESPEC

#endif
