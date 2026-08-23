' mezura-expect lines=13 code=8 comments=2 extra=3
Imports System

''' <summary>A doc comment</summary>
Module Greeter

    Sub Main()
        Dim quoted As String = "he said ""hello"" and left"
        Dim tricky As String = "an ' inside opens no comment"
        Console.WriteLine(quoted & tricky)   ' a trailing comment
    End Sub

End Module
