// mezura-expect lines=11 code=5 comments=3 extra=3 structs=1
uniform float time;

/* a block
   comment */
struct Light { vec3 pos; };

void main() {
    gl_FragColor = vec4(1.0);
}
#line 1 "// not a comment.glsl"
