"mezura-expect lines=24 code=13 comments=4 extra=7"
"
I count, and this class comment
says ""hello"" with its quotes doubled.
"
Class {
	#name : #Counter,
	#superclass : #Object,
	#instVars : [ 'count' ],
	#category : #'Demo-Core'
}

{ #category : #accessing }
Counter >> increment [
	"Add one, which is what it's for"
	count := count + 1. "a trailing comment"
	^ 'it''s "not a comment"'
]

{ #category : #printing }
Counter >> printOn: aStream [
	aStream nextPut: $"
	aStream print: count
]
