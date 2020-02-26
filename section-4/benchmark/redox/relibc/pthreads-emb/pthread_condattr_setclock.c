#include "pthread.h"
#include "implement.h"

int
pthread_condattr_setclock (pthread_condattr_t * attr, clockid_t clock_id)
{
  int result;

  if ((attr != NULL && *attr != NULL)
      && ((clock_id == CLOCK_REALTIME)
          || (clock_id == CLOCK_MONOTONIC)))
    {
      (*attr)->clock_id = clock_id;
      result = 0;
    }
  else
    {
      result = EINVAL;
    }

  return result;
}
