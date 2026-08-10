# mezura-expect lines=14 code=6 comments=6 extra=2 functions=1
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
