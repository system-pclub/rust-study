/* pte_types.h  */

#ifndef PTE_TYPES_H
#define PTE_TYPES_H

#include <time.h>

typedef int pid_t;

struct timespec
{
  time_t  tv_sec;
  long    tv_nsec;   
};

typedef unsigned int mode_t;


struct timeb
{ 
  time_t time;
  unsigned short millitm;
  short timezone;
  short dstflag;
};

#endif /* PTE_TYPES_H */
