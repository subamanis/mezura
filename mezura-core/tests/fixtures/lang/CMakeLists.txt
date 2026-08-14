# mezura-expect lines=15 code=7 comments=5 extra=3 functions=1
cmake_minimum_required(VERSION 3.20)
project(app)

#[[ a bracket comment
    over two lines ]]
add_executable(app main.cpp)

#[==[ a level two one
holding ]] as text
]==]
function(configure target)
    target_compile_features(${target} PUBLIC cxx_std_20)
endfunction()
set(NOTE "# not a comment")
