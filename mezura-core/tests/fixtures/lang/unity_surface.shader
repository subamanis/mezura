// mezura-expect lines=13 code=5 comments=3 extra=5 shaders=1 subshaders=1 passes=1
Shader "Custom/Surface"
{
    Properties { _Color ("Color", Color) = (1,1,1,1) }
    /* the only pass
       there is */
    SubShader
    {
        Pass { Name "FORWARD" }
    }

    FallBack "Diffuse"
}
