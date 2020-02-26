.PHONY: all
all: openssl
	@:  # No-op to silence the "make: Nothing to be done for 'all'." message.

# From https://wiki.openssl.org/index.php/Android
.PHONY: openssl
openssl: openssl-${OPENSSL_VERSION}/libssl.so

openssl-${OPENSSL_VERSION}/libssl.so: openssl-${OPENSSL_VERSION}/Configure
	./openssl.sh ${ANDROID_NDK} ${OPENSSL_VERSION}

openssl-${OPENSSL_VERSION}/Configure:
	URL=https://servo-deps.s3.amazonaws.com/android-deps/openssl-${OPENSSL_VERSION}.tar.gz; \
	curl $$URL | tar xzf -
