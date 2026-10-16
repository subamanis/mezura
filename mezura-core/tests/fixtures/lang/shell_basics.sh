# mezura-expect lines=12 code=7 comments=3 extra=2 functions=1
echo hello

# a comment
function greet {
  echo world
  echo "quoted" 'raw'
}
cat <<EOF
it's a heredoc
EOF
# a comment after it
