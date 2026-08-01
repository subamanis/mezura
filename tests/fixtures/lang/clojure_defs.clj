; mezura-expect lines=11 code=6 comments=2 extra=3 functions=2 macros=1 namespaces=1
(ns example.core)

(defn greet [name]
  (str "hello " name))

(defmacro unless [test body]
  (list 'if (list 'not test) body))

; a trailing comment
(defn bye [] (greet "bye"))
