# mezura-expect lines=17 code=10 comments=5 extra=2
<#
    a block comment
#>
function Get-Greeting {
    param([string]$Name)
    "hello $Name"
}

# a line comment
Get-Greeting -Name 'world'
$a = @"
a double here string
"@
$b = @'
a single here string
'@
