.PHONY: all
all:
	mkdir -p build generated
	printf x > generated/payload
	touch -d @946684800 generated/payload
	tar -cf build/out.tar generated/payload
