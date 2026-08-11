<!-- mezura-real    21 lines 9 code 12 comment -->
<!-- mezura-count   21 lines 9 code 12 comment -->
<!-- mezura-section TypeScript 2 lines 1 code 1 comment -->
<!-- mezura-section SCSS 2 lines 1 code 1 comment -->
<!-- tokei-real     21 lines 9 code 12 comment -->
<!-- tokei-count    21 lines 9 code 12 comment -->
<!-- trap: the opener names the section's language, so this file is Vue, TypeScript and SCSS and
     not Vue, JavaScript and CSS. The three totals are the same either way, which is why a case
     about labelling has to declare its sections and not only its counts -->
<!-- tokei: counts the same lines and calls the two sections JavaScript and CSS -->
<template>
  <p>x</p>
</template>
<script lang="ts">
// a ts comment
interface Point { x: number }
</script>
<style lang="scss">
// an scss line comment, which plain css does not have
.a { .b { color: red; } }
</style>
