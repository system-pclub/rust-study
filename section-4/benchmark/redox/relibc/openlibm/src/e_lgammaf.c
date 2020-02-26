/* e_lgammaf.c -- float version of e_lgamma.c.
 * Conversion to float by Ian Lance Taylor, Cygnus Support, ian@cygnus.com.
 */

/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 */

#include "cdefs-compat.h"
//__FBSDID("$FreeBSD: src/lib/msun/src/e_lgammaf.c,v 1.8 2008/02/22 02:30:35 das Exp $");

/* __ieee754_lgammaf(x)
 * Return the logarithm of the Gamma function of x.
 *
 * Method: call __ieee754_lgammaf_r
 */

#include <openlibm_math.h>

#include "math_private.h"

OLM_DLLEXPORT float
__ieee754_lgammaf(float x)
{
#ifdef OPENLIBM_ONLY_THREAD_SAFE
	int signgam;
#endif

	return __ieee754_lgammaf_r(x,&signgam);
}
