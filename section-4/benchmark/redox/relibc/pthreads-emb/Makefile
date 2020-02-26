SRC=$(wildcard *.c)
OBJ=$(patsubst %.c,%.o,$(SRC))

all: libpthread.a

libpthread.a: $(OBJ)
	$(AR) -rcs $@ $(OBJ)

libpthread.so: $(OBJ)
	$(CC) $(CFLAGS) -nostdlib -shared -o $@ $(OBJ)

%.o: %.c
	$(CC) $(CFLAGS) -fPIC -I . -c $< -o $@
