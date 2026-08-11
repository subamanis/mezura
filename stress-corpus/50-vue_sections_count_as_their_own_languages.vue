<!-- mezura-real    22 lines 9 code 12 comment -->
<!-- mezura-count   22 lines 9 code 12 comment -->
<!-- mezura-section JavaScript 3 lines 1 code 1 comment -->
<!-- mezura-section CSS 2 lines 1 code 1 comment -->
<!-- tokei-real     22 lines 9 code 12 comment -->
<!-- tokei-count    22 lines 10 code 11 comment -->
<!-- trap: one file, three languages. The script and style sections are counted with their own
     symbols, so the js comment and the css comment are comments although the shell knows neither -->
<!-- tokei: the apostrophe of "it's" opens a string, and the template comment under it is code -->
<template>
  <p class="a">it's fine</p>
  <!-- a template comment -->
</template>
<script>
// a js comment

var x = "</p>";
</script>
<style>
/* a css comment */
.a { color: red; }
</style>
