# mezura-expect lines=19 code=12 comments=3 extra=4 classes=1 functions=2 signals=1
class_name Player

signal died(where)

# how fast the thing goes
var speed := 220.0

func _ready() -> void:
	speed = 220.0

func hurt(amount: int) -> void:
	# nothing yet
	speed -= amount
var s = "# not a comment"
var t = 'also'
var doc = """
a block string
"""
