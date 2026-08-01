# mezura-expect lines=11 code=4 comments=5 extra=2
<#
    a block comment
#>
function Get-Greeting {
    param([string]$Name)
    "hello $Name"
}

# a line comment
Get-Greeting -Name 'world'
