import androidx.annotation.VisibleForTesting
import androidx.room.Entity
import androidx.room.Ignore

/* A reader with no @Test in its code, and Room's @Ignore on a field. */
@Entity
class Parser {
    @Ignore
    var cached = ""

    val doc = "@Test marks a test"
    val example = """
        @Test
        fun t() {}
    """

    @VisibleForTesting
    fun parse(text: String): String = text // @Test
}
