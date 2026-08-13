# mezura-expect lines=15 code=9 comments=3 extra=3 includes=1
include config.mk

CC = gcc
FLAGS = -Wall -O2

# build everything
all: main.o
	$(CC) $(FLAGS) -o app main.o

# clean up
clean:
	rm -f app
NOTE = "# not a comment"
ALT = 'also'
