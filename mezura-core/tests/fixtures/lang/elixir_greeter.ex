# mezura-expect lines=13 code=10 comments=2 extra=1 modules=1 functions=2
defmodule Greeter do
  @doc """
  a heredoc
  """
  @note '''
  another heredoc
  '''
  def hello, do: "# not a comment"
  defp secret, do: 'also'

  # a comment
end
