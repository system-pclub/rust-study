#include "pthread.h"
#include "implement.h"

int
pthread_condattr_getclock (const pthread_condattr_t * attr, clockid_t *clock_id)
{
  int result;

  if ((attr != NULL && *attr != NULL) && (clock_id != NULL))
    {
      *clock_id = (*attr)->clock_id;
      result = 0;
    }
  else
    {
      result = EINVAL;
    }

  return result;
}
