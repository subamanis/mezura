# mezura-expect lines=15 code=7 comments=3 extra=5 resources=1 variables=1 outputs=1
variable "region" {
  default = "eu-west-1"   // a trailing comment
}

/* a block comment
   over two lines */
resource "aws_s3_bucket" "logs" {
  bucket = "logs-# not a comment /* nor this */"
  tags = {
    Name = "logs"
  }
}

output "arn" { value = aws_s3_bucket.logs.arn }
