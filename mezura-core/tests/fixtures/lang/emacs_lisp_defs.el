; mezura-expect lines=9 code=6 comments=1 extra=2 functions=1 macros=1
(defun mezura-hello (name)
  "Say hello to NAME.

Longer docstring line."
  (message "hello %s" name))

(defmacro mezura-when-let (spec &rest body)
  `(let (,spec) ,@body))
