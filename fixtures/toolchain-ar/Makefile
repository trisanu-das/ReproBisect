all: build/libdemo.a

build/main.o: main.c
	mkdir -p build
	$(CC) -c -O0 -o $@ $<

build/libdemo.a: build/main.o
	$(AR) rcs $@ $<
