//44 lines (13 code) , 3 classes

public class Main
{
	public static void testKeywordClass() 
	{
		//--- invalids ---
		//class
		// class
		/*
		class
		*/
		var class1 = " class " ;
		
		int i = 2;
		println(i.class);
		
		//--- valids ---
		class Point {int x = 0; int y = 0;}
		
		/*" class */class//
	}
	
	pubic static void testCommentsAndCodeLines() 
	{
		i//
		{//
		}//
		/* fdf */
		/**/i/*
		frgdf
		"
		*/
		int a = 2;
	}
	
	public static testStringsAndComments() 
	{
		String s = " fdf /* fdf ";
		/*
		"
		*/
	}
}