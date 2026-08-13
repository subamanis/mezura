! mezura-expect lines=13 code=9 comments=2 extra=2
module greetings
    implicit none
contains

    ! greets
    subroutine greet(name)
        character(len=*) :: name
        print *, "hello ", name
    end subroutine greet

end module greetings
character(len=*), parameter :: note = '! not a comment'
